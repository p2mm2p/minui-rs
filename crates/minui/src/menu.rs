//! 菜单导航状态机
//!
//! 对应原版 C `minui.c` 的菜单区（`openDirectory`/`closeDirectory`/
//! `Entry_open`/`loadLast`，minui.c:1138-1296）。本模块承载"用户如何
//! 导航目录"的全部状态与逻辑：
//!
//! - `Menu` 结构体收拢 C 的全局状态（目录栈、can_resume/should_resume、
//!   show_version、restore 五字段）
//! - 滚动窗口（UP/DOWN/LEFT/RIGHT 边界）与字母组跳转（L1/R1）为**纯函数**
//!   （`scroll`/`alpha_jump`）——可独立测试
//! - `load_last` 从 `/tmp/last.txt` 恢复上次浏览位置（消费
//!   `launch::LAST_PATH`）
//!
//! ## 依赖方向
//!
//! 本模块 SHALL 只依赖 `browser`（`Directory`/`Entry`）、`launch`
//! （`open_rom`/`open_pak`/`save_last`/`LAST_PATH`）、`recents`
//! （`find_recents`）与 `common`——不依赖 `render`/平台 crate
//! （`mod_keys`/`sleep_btn` 等平台值由调用方经参数传入）。
//! 依赖方向单向：`menu → launch`（launch 不知 menu 存在）。
//!

#![cfg_attr(not(feature = "platform-tg5040"), allow(dead_code))] // 无平台 feature（测试编译）时保留 allow
//!
//! 「menu 导航状态机」

use common::paths::{get_collections_path, get_faux_recent_path, get_roms_path};
use common::utils::{exact_match, exists, get_file, prefix_match, suffix_match};

use crate::browser::{Directory, EntryType};
use crate::disc;
use crate::launch::{self, LAST_PATH};
use crate::recents::find_recents;

/// 菜单导航状态机
///
/// 收拢 C 的全局状态（`top`+`stack` 栈、`can_resume`、`should_resume`、
/// `show_version`、restore 五字段），提供打开/关闭目录、条目分发、
/// 浏览位置恢复。`rows` 为每页行数（对应 C `MAIN_ROW_COUNT` 宏——
/// 平台布局值经 `Menu::new` 参数化传入，本模块保持平台无关）。
///
/// 生命周期对应 C `Menu_init`（minui.c:1286-1291）/`Menu_quit`（:1293-1296）：
/// `Menu::new` 打开 SDCARD 根目录并恢复上次浏览位置。
pub(crate) struct Menu {
    /// 打开的目录栈（栈顶 = 当前浏览目录，对应 C `top` + `stack`）
    stack: Vec<Directory>,
    /// 每页可显示行数（`Platform::main_row_count()`，滚动窗口计算用）
    rows: usize,
    /// 当前条目是否可续玩（`launch::ready_resume` 的结果，X 键可用性）
    can_resume: bool,
    /// 续玩意图（C `should_resume`——X 键触发后由 `entry_open` 一次性消费）
    should_resume: bool,
    /// 是否显示版本页
    show_version: bool,
    /// restore 状态：关闭目录时保存，打开目录时恢复（C :432-435 五个全局）
    restore_depth: usize,
    /// restore 状态：关闭时栈顶的 selected（C `restore_relative`）
    restore_relative: usize,
    /// restore 状态：关闭时被关目录的 selected
    restore_selected: usize,
    /// restore 状态：关闭时被关目录的 start
    restore_start: usize,
    /// restore 状态：关闭时被关目录的 end
    restore_end: usize,
}

impl Menu {
    /// 创建菜单（对应 C `Menu_init`，minui.c:1286-1291）
    ///
    /// 打开 SDCARD 根目录后调用 `load_last` 恢复上次浏览位置。
    ///
    /// # 参数
    ///
    /// - `sdcard_path`:SD 卡根路径（`Platform::SDCARD_PATH`）
    /// - `platform`:平台代码（`Platform::PLATFORM`）
    /// - `paks_path`:平台 paks 目录（`common::paths::get_paks_path`）
    /// - `rows`:每页行数（`Platform::main_row_count()`）
    pub fn new(sdcard_path: &str, platform: &str, paks_path: &str, rows: usize) -> Self {
        let mut menu = Self {
            stack: Vec::new(),
            rows,
            can_resume: false,
            should_resume: false,
            show_version: false,
            restore_depth: 0,
            restore_relative: 0,
            restore_selected: 0,
            restore_start: 0,
            restore_end: 0,
        };
        menu.open_directory(sdcard_path, false, sdcard_path, platform, paks_path);
        menu.load_last(sdcard_path, platform, paks_path);
        menu
    }

    /// 当前浏览目录（栈顶）
    pub fn top(&self) -> &Directory {
        self.stack.last().expect("目录栈不应为空")
    }

    /// 当前浏览目录（可变引用）
    fn top_mut(&mut self) -> &mut Directory {
        self.stack.last_mut().expect("目录栈不应为空")
    }

    /// 目录栈深度（根目录为 1——对应 C `stack->count`）
    pub fn stack_depth(&self) -> usize {
        self.stack.len()
    }

    /// 当前条目是否可续玩（X 键可用性）
    pub fn can_resume(&self) -> bool {
        self.can_resume
    }

    /// 是否显示版本页
    pub fn show_version(&self) -> bool {
        self.show_version
    }

    /// 设置版本页显示状态
    pub fn set_show_version(&mut self, v: bool) {
        self.show_version = v;
    }

    /// 标记续玩意图（X 键触发，`entry_open` 消费）
    pub fn request_resume(&mut self) {
        self.should_resume = true;
    }

    /// 刷新当前条目的可续玩状态（对应 C `readyResume`，minui.c:1026-1028）
    ///
    /// 由主循环在脏帧/目录变化后调用。
    pub fn refresh_resume(&mut self, sdcard_path: &str) {
        let top = self.top();
        if let Some(entry) = top.entries.get(top.selected) {
            let is_dir = entry.entry_type == EntryType::Dir;
            self.can_resume = launch::ready_resume(&entry.path, is_dir, sdcard_path);
        } else {
            self.can_resume = false;
        }
    }

    /// 应用滚动/跳转结果（主循环消费 `scroll`/`alpha_jump` 的返回值）
    ///
    /// 修改栈顶目录的 `selected`/`start`/`end`。
    pub fn move_to(&mut self, selected: usize, start: usize, end: usize) {
        let top = self.top_mut();
        top.selected = selected;
        top.start = start;
        top.end = end;
    }

    /// 打开目录（对应 C `openDirectory`，minui.c:1138-1174）
    ///
    /// `auto_launch=true` 时先检测 cue/m3u 直接启动（不入栈）；否则创建
    /// `Directory` 入栈，`selected`/`start`/`end` 按 restore 状态恢复。
    ///
    /// # 返回值
    ///
    /// `true` = 已直接启动（auto_launch 命中，调用方应退出主循环）；
    /// `false` = 目录已入栈。
    pub fn open_directory(
        &mut self,
        path: &str,
        auto_launch: bool,
        sdcard_path: &str,
        platform: &str,
        paks_path: &str,
    ) -> bool {
        // auto_launch：cue 直接启动（C :1139-1143）
        if auto_launch {
            if let Some(cue) = disc::find_cue(path) {
                self.start_rom(&cue, false, None, sdcard_path, platform, paks_path);
                launch::save_last(false, path, sdcard_path);
                return true;
            }
            // m3u 直接启动（C :1145-1156——`{dir}/{dir_name}.m3u`）
            if let Some(m3u) = dir_m3u(path)
                && let Some(disc_path) = disc::get_first_disc(&m3u)
            {
                self.start_rom(&disc_path, false, None, sdcard_path, platform, paks_path);
                launch::save_last(false, path, sdcard_path);
                return true;
            }
        }

        // restore 恢复（C :1158-1167）
        let mut selected = 0usize;
        let mut start = 0usize;
        let mut end = 0usize;
        if !self.stack.is_empty()
            && self.restore_depth == self.stack.len()
            && self.top().selected == self.restore_relative
        {
            selected = self.restore_selected;
            start = self.restore_start;
            end = self.restore_end;
        }

        let mut dir = Directory::new(path, selected, sdcard_path, platform, paks_path);
        dir.start = start;
        dir.end = if end != 0 {
            end
        } else {
            dir.entries.len().min(self.rows)
        };
        self.stack.push(dir);
        false
    }

    /// 关闭目录（对应 C `closeDirectory`，minui.c:1175-1183）
    ///
    /// 保存当前目录的 restore 状态后出栈。
    pub fn close_directory(&mut self) {
        if self.stack.len() <= 1 {
            return; // 根目录不可关闭（C 由调用方 stack->count>1 守卫）
        }
        // 先提取值再出栈（避免借用 self 与赋值冲突）
        let (selected, start, end) = {
            let top = self.top();
            (top.selected, top.start, top.end)
        };
        self.restore_selected = selected;
        self.restore_start = start;
        self.restore_end = end;
        self.stack.pop();
        self.restore_depth = self.stack.len();
        self.restore_relative = self.top().selected;
    }

    /// 打开条目（对应 C `Entry_open`，minui.c:1185-1208）
    ///
    /// 按类型分发：Rom → `launch::open_rom`（`should_resume` 消费）；
    /// Pak → `launch::open_pak`；Dir → `open_directory(path, true)`。
    /// 启动后保存浏览位置（`launch::save_last`）。
    ///
    /// # 返回值
    ///
    /// `true` = 已启动或已下钻（Rom/Pak 启动后调用方退出；Dir 下钻返回
    /// auto_launch 结果）；`false` = 无动作（不应发生）。
    pub fn entry_open(
        &mut self,
        index: usize,
        sdcard_path: &str,
        platform: &str,
        paks_path: &str,
    ) -> bool {
        let entry = self.top().entries[index].clone();
        let recent_alias = entry.name.clone();
        match entry.entry_type {
            EntryType::Rom => {
                // Collections 下启动：last 记录合集路径（C :1189-1199）
                let last = if prefix_match(&get_collections_path(sdcard_path), &self.top().path) {
                    let filename = entry.path.rsplit('/').next().unwrap_or("");
                    Some(format!("{}/{filename}", self.top().path))
                } else {
                    None
                };
                let resume = self.should_resume;
                self.should_resume = false; // 一次性消费（C :1101）
                self.start_rom(
                    &entry.path,
                    resume,
                    Some(&recent_alias),
                    sdcard_path,
                    platform,
                    paks_path,
                );
                let top_is_recents =
                    exact_match(&get_faux_recent_path(sdcard_path), &self.top().path);
                launch::save_last(
                    top_is_recents,
                    last.as_deref().unwrap_or(&entry.path),
                    sdcard_path,
                );
                true
            }
            EntryType::Pak => {
                let mut recents =
                    find_recents(sdcard_path, platform, paks_path).unwrap_or_default();
                launch::open_pak(&entry.path, &mut recents, sdcard_path, platform, paks_path);
                let top_is_recents =
                    exact_match(&get_faux_recent_path(sdcard_path), &self.top().path);
                launch::save_last(top_is_recents, &entry.path, sdcard_path);
                true
            }
            EntryType::Dir => {
                self.open_directory(&entry.path, true, sdcard_path, platform, paks_path)
            }
        }
    }

    /// 启动 ROM（`find_recents` 现查 + `launch::open_rom`，决策 3）
    fn start_rom(
        &mut self,
        path: &str,
        resume: bool,
        alias: Option<&str>,
        sdcard_path: &str,
        platform: &str,
        paks_path: &str,
    ) {
        let mut recents = find_recents(sdcard_path, platform, paks_path).unwrap_or_default();
        launch::open_rom(
            path,
            resume,
            alias,
            &mut recents,
            sdcard_path,
            platform,
            paks_path,
        );
    }

    /// 恢复上次浏览位置（对应 C `loadLast`，minui.c:1222-1282）
    ///
    /// 读 `launch::LAST_PATH`（`/tmp/last.txt`）→ 路径逐级分解 →
    /// 在当前栈顶目录匹配条目（exact / collated 前缀 / Collections
    /// 文件名后缀三分支，C :1259）→ 恢复 selected/start/end →
    /// 目录条目下钻（`open_directory`）。
    fn load_last(&mut self, sdcard_path: &str, platform: &str, paks_path: &str) {
        let Some(content) = get_file(LAST_PATH) else {
            return;
        };
        let last_path = content.trim().to_string();

        // 路径逐级分解（从最深到根，C :1236-1242）
        let mut segments: Vec<String> = Vec::new();
        let mut current = last_path.clone();
        while !exact_match(&current, sdcard_path) {
            segments.push(current.clone());
            match current.rsplit_once('/') {
                Some((parent, _)) => current = parent.to_string(),
                None => break,
            }
        }

        // 逆序（从浅到深）逐级下钻（C :1244-1279）
        while let Some(path) = segments.pop() {
            if exact_match(&path, &get_roms_path(sdcard_path)) {
                continue; // romsDir 是有效根（C :1246）
            }
            // collated 前缀（仅 console 目录且以 ')' 结尾，C :1249-1253）
            let mut collated = String::new();
            if suffix_match(")", &path)
                && path
                    .rsplit_once('/')
                    .map(|(parent, _)| exact_match(&get_roms_path(sdcard_path), parent))
                    .unwrap_or(false)
            {
                collated = path.clone();
                if let Some(paren) = collated.rfind('(') {
                    collated.truncate(paren + 1);
                }
            }
            let filename = path.rsplit('/').next().unwrap_or("").to_string();

            // 三分支匹配（C :1259）
            let mut matched: Option<usize> = None;
            for (i, entry) in self.top().entries.iter().enumerate() {
                let exact = exact_match(&entry.path, &path);
                let collated_hit = !collated.is_empty() && prefix_match(&collated, &entry.path);
                let collection_hit = prefix_match(&get_collections_path(sdcard_path), &last_path)
                    && suffix_match(&filename, &entry.path);
                if exact || collated_hit || collection_hit {
                    matched = Some(i);
                    break;
                }
            }
            let Some(i) = matched else {
                continue;
            };

            // 恢复 selected/start/end（C :1260-1268）
            let rows = self.rows;
            let entry = {
                let top = self.top_mut();
                top.selected = i;
                if i >= top.end {
                    top.start = i;
                    top.end = top.start + rows;
                    if top.end > top.entries.len() {
                        top.end = top.entries.len();
                        top.start = top.end.saturating_sub(rows);
                    }
                }
                top.entries[i].clone()
            };

            // 最后一层且非 Recently/Collections 子目录 → 不显示内容（C :1269）
            let is_recent = exact_match(&entry.path, &get_faux_recent_path(sdcard_path));
            let is_collection_child = prefix_match(&get_collections_path(sdcard_path), &entry.path)
                && !exact_match(&entry.path, &get_collections_path(sdcard_path));
            if segments.is_empty() && !is_recent && !is_collection_child {
                break;
            }
            if entry.entry_type == EntryType::Dir {
                self.open_directory(&entry.path, false, sdcard_path, platform, paks_path);
            }
        }
    }
}

/// 目录级 m3u 路径（`{dir}/{dir_name}.m3u`）
///
/// 对应 C `openDirectory` 的 m3u 判定（minui.c:1145-1148——`hasCue`
/// 输出的路径替换扩展名为 `.m3u`；Rust 版直接构造，不复制 C 对
/// 未初始化 `auto_path` 的读取缺陷）。
fn dir_m3u(dir_path: &str) -> Option<String> {
    let trimmed = dir_path.trim_end_matches('/');
    let dir_name = trimmed.rsplit_once('/').map(|(_, d)| d)?;
    let m3u = format!("{trimmed}/{dir_name}.m3u");
    exists(&m3u).then_some(m3u)
}

/// 滚动方向（对应 C `PAD_justRepeated` 的 UP/DOWN/LEFT/RIGHT 分支）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollDir {
    Up,
    Down,
    Left,
    Right,
}

/// 滚动窗口计算（纯函数）
///
/// 对应 C minui.c:1367-1427 的 UP/DOWN/LEFT/RIGHT 边界逻辑。返回
/// `(selected, start, end)`：
///
/// - 顶部/底部停止：`from_press=false`（重复触发）时 selected 到顶/底
///   则不动（C :1368-1370/:1386-1388）
/// - 回绕：`from_press=true`（刚按下）时越界回绕并整页对齐窗口
///   （C :1373-1378/:1391-1394）
/// - Left/Right：整页移动（`±rows`），越界时夹到首/尾并整页对齐
///   （C :1402-1427）
///
/// # 参数
///
/// - `selected`:当前选中条目下标
/// - `start`/`end`:滚动窗口（页首/页尾，开区间）
/// - `total`:条目总数（调用方保证 > 0）
/// - `rows`:每页行数（`Platform::main_row_count()`）
/// - `dir`:滚动方向
/// - `from_press`:本次是否由"刚按下"触发（非重复触发）——顶部/底部
///   允许回绕
///
/// # 返回值
///
/// 滚动后的 `(selected, start, end)`。
pub(crate) fn scroll(
    selected: usize,
    start: usize,
    end: usize,
    total: usize,
    rows: usize,
    dir: ScrollDir,
    from_press: bool,
) -> (usize, usize, usize) {
    let s = selected as i32;
    let st = start as i32;
    let en = end as i32;
    let t = total as i32;
    let r = rows as i32;
    let (ns, nst, nen): (i32, i32, i32) = match dir {
        ScrollDir::Up => {
            if s == 0 && !from_press {
                (s, st, en)
            } else {
                let ns = s - 1;
                if ns < 0 {
                    // 回绕到底部（C :1373-1378）
                    let nst = (t - r).max(0);
                    (t - 1, nst, t)
                } else if ns < st {
                    // 窗口逐行上移（C :1379-1382）
                    (ns, st - 1, en - 1)
                } else {
                    (ns, st, en)
                }
            }
        }
        ScrollDir::Down => {
            if s == t - 1 && !from_press {
                (s, st, en)
            } else {
                let ns = s + 1;
                if ns >= t {
                    // 回绕到顶部（C :1391-1394）
                    (0, 0, t.min(r))
                } else if ns >= en {
                    // 窗口逐行下移（C :1396-1399）
                    (ns, st + 1, en + 1)
                } else {
                    (ns, st, en)
                }
            }
        }
        ScrollDir::Left => {
            let ns = s - r;
            if ns < 0 {
                // 越界夹到顶部（C :1403-1408）
                (0, 0, t.min(r))
            } else if ns < st {
                // 窗口整页回退（C :1409-1413）
                let nst = (st - r).max(0);
                (ns, nst, nst + r)
            } else {
                (ns, st, en)
            }
        }
        ScrollDir::Right => {
            let ns = s + r;
            if ns >= t {
                // 越界夹到底部（C :1417-1422）
                let nst = (t - r).max(0);
                (t - 1, nst, t)
            } else if ns >= en {
                // 窗口整页前进（C :1423-1427）
                let nen = (en + r).min(t);
                (ns, nen - r, nen)
            } else {
                (ns, st, en)
            }
        }
    };
    (ns as usize, nst as usize, nen as usize)
}

/// 字母组跳转（L1/R1，纯函数）
///
/// 对应 C minui.c:1431-1456。以 `alphas` 数组（`Directory::alphas`）与
/// 当前条目所属组号 `current_alpha`（`entry.alpha`）为输入，跳转到
/// 上一组（`next=false`）或下一组（`next=true`）的起始条目，并把滚动
/// 窗口对齐到该条目（`total > rows` 时，C :1436-1441/:1449-1454）。
///
/// # 参数
///
/// - `alphas`:字母组起始条目下标数组（`Directory::alphas`）
/// - `current_alpha`:当前条目所属字母组在 `alphas` 中的下标
/// - `total`:条目总数
/// - `rows`:每页行数
/// - `next`:`false` = 上一组（L1），`true` = 下一组（R1）
///
/// # 返回值
///
/// `Some((selected, start, end))`——跳转成功；`None`——已在边界组
/// （C 的 `if (i>=0)`/`if (i<count)` 守卫不通过，"什么都不做"，
/// 调用方不应用跳转）。
pub(crate) fn alpha_jump(
    alphas: &[usize],
    current_alpha: usize,
    total: usize,
    rows: usize,
    next: bool,
) -> Option<(usize, usize, usize)> {
    let target = if next {
        current_alpha.checked_add(1)?
    } else {
        current_alpha.checked_sub(1)?
    };
    let selected = *alphas.get(target)?;
    if total > rows {
        let mut end = selected + rows;
        if end > total {
            end = total;
        }
        let start = end - rows;
        Some((selected, start, end))
    } else {
        Some((selected, 0, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── scroll：顶部/底部停止与回绕（spec 场景「滚动顶部停止与回绕」）──

    #[test]
    fn scroll_up_stops_at_top_on_repeat() {
        // from_press=false：顶部停止（C :1368-1370）
        assert_eq!(scroll(0, 0, 8, 20, 8, ScrollDir::Up, false), (0, 0, 8));
    }

    #[test]
    fn scroll_up_wraps_on_press() {
        // from_press=true：回绕到底部，窗口整页对齐（C :1373-1378）
        assert_eq!(scroll(0, 0, 8, 20, 8, ScrollDir::Up, true), (19, 12, 20));
    }

    #[test]
    fn scroll_down_stops_at_bottom_on_repeat() {
        // from_press=false：底部停止（C :1386-1388）
        assert_eq!(
            scroll(19, 12, 20, 20, 8, ScrollDir::Down, false),
            (19, 12, 20)
        );
    }

    #[test]
    fn scroll_down_wraps_on_press() {
        // from_press=true：回绕到顶部（C :1391-1394）
        assert_eq!(scroll(19, 12, 20, 20, 8, ScrollDir::Down, true), (0, 0, 8));
    }

    #[test]
    fn scroll_wrap_with_short_list() {
        // total < rows：回绕窗口 = 全列表
        assert_eq!(scroll(0, 0, 4, 4, 8, ScrollDir::Up, true), (3, 0, 4));
    }

    // ── scroll：窗口逐行移动（spec 场景「滚动窗口逐行移动」）──

    #[test]
    fn scroll_down_window_slides() {
        // selected 越出 end → start/end 同步 +1（C :1396-1399）
        assert_eq!(scroll(8, 0, 8, 20, 8, ScrollDir::Down, false), (9, 1, 9));
    }

    #[test]
    fn scroll_up_window_slides() {
        // selected 越出 start → start/end 同步 -1（C :1379-1382）
        assert_eq!(scroll(8, 0, 8, 20, 8, ScrollDir::Up, false), (7, 0, 8));
    }

    #[test]
    fn scroll_down_within_window() {
        // selected 在窗口内 → 仅 selected 变化
        assert_eq!(scroll(3, 0, 8, 20, 8, ScrollDir::Down, false), (4, 0, 8));
    }

    // ── scroll：整页移动（spec 场景「滚动整页移动」）──

    #[test]
    fn scroll_right_page_jump() {
        // 整页 +8（C :1415-1427）
        assert_eq!(scroll(2, 0, 8, 20, 8, ScrollDir::Right, false), (10, 8, 16));
    }

    #[test]
    fn scroll_right_clamps_to_end() {
        // 越界夹到末尾，窗口回退对齐（C :1417-1422）
        assert_eq!(
            scroll(18, 16, 20, 20, 8, ScrollDir::Right, false),
            (19, 12, 20)
        );
    }

    #[test]
    fn scroll_left_page_jump() {
        assert_eq!(scroll(10, 8, 16, 20, 8, ScrollDir::Left, false), (2, 0, 8));
    }

    #[test]
    fn scroll_left_clamps_to_start() {
        // 越界夹到顶部（C :1403-1408）
        assert_eq!(scroll(2, 0, 8, 20, 8, ScrollDir::Left, false), (0, 0, 8));
    }

    #[test]
    fn scroll_left_window_step_back() {
        // selected 未越界但 < start：窗口整页回退并夹 0（C :1409-1413）
        assert_eq!(scroll(8, 8, 16, 20, 8, ScrollDir::Left, false), (0, 0, 8));
    }

    #[test]
    fn scroll_right_short_list() {
        // total < rows：Right 越界夹到末尾（窗口 = 全列表）
        assert_eq!(scroll(2, 0, 4, 4, 8, ScrollDir::Right, false), (3, 0, 4));
    }

    // ── alpha_jump（spec 场景「alpha 跳转」）──

    #[test]
    fn alpha_jump_previous_group() {
        // alphas=[0,4,9]，当前组 1 → 上一组 alphas[0]=0，窗口对齐（C :1431-1441）
        assert_eq!(alpha_jump(&[0, 4, 9], 1, 20, 8, false), Some((0, 0, 8)));
    }

    #[test]
    fn alpha_jump_next_group() {
        // 下一组 alphas[2]=9 → 窗口 (9, 17)
        assert_eq!(alpha_jump(&[0, 4, 9], 1, 20, 8, true), Some((9, 9, 17)));
    }

    #[test]
    fn alpha_jump_first_group_no_op() {
        // 已是最前组 → None（C 的 i>=0 守卫）
        assert_eq!(alpha_jump(&[0, 4, 9], 0, 20, 8, false), None);
    }

    #[test]
    fn alpha_jump_last_group_no_op() {
        // 已是最后组 → None（C 的 i<count 守卫）
        assert_eq!(alpha_jump(&[0, 4, 9], 2, 20, 8, true), None);
    }

    #[test]
    fn alpha_jump_short_list_no_realign() {
        // total <= rows：窗口不重对齐（保持全列表）
        assert_eq!(alpha_jump(&[0, 2, 4], 1, 4, 8, true), Some((4, 0, 4)));
    }

    #[test]
    fn alpha_jump_end_clamped_window() {
        // 窗口对齐时 end 夹到 total（C :1439-1441）
        assert_eq!(alpha_jump(&[0, 4, 15], 1, 20, 8, true), Some((15, 12, 20)));
    }

    // ── 模块头注释引用的依赖（编译期验证，避免误删 import）──

    #[test]
    fn imports_resolve() {
        // 编译期验证：browser/disc/launch/common 路径可解析
        let _ = std::any::type_name::<Directory>();
        let _ = std::any::type_name::<crate::browser::Entry>();
        let _ = std::any::type_name::<EntryType>();
        assert!(matches!(ScrollDir::Up, ScrollDir::Up));
    }

    // ── Menu：目录栈（spec 场景「open_directory auto_launch」「entry_open 分发」）──

    use std::fs;

    fn temp_root(name: &str) -> String {
        let base = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        format!("{base}/minui_menu_{}_{name}", std::process::id())
    }

    fn setup_sdcard(root: &str) {
        fs::create_dir_all(format!("{root}/.userdata/shared/.minui")).unwrap();
        fs::create_dir_all(format!("{root}/Roms")).unwrap();
    }

    fn install_emu(root: &str, platform: &str, emu: &str) {
        let pak = format!("{root}/Emus/{platform}/{emu}.pak/launch.sh");
        fs::create_dir_all(std::path::Path::new(&pak).parent().unwrap()).unwrap();
        fs::write(&pak, "#!/bin/sh\n").unwrap();
    }

    fn paks_path(root: &str, platform: &str) -> String {
        format!("{root}/.system/{platform}/paks")
    }

    const PLATFORM: &str = "tg5040";
    const ROWS: usize = 8;

    fn lock_tmp() -> std::sync::MutexGuard<'static, ()> {
        // 跨模块共享锁（见 crate::test_util）——/tmp 协议文件全局共享
        let guard = crate::test_util::TMP_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _ = common::utils::remove_file(crate::launch::LAST_PATH);
        let _ = common::utils::remove_file(common::paths::CHANGE_DISC_PATH);
        let _ = common::utils::remove_file("/tmp/next");
        guard
    }

    #[test]
    fn menu_new_opens_root() {
        let root = temp_root("new_root");
        setup_sdcard(&root);
        let _guard = lock_tmp();
        let menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        assert_eq!(menu.stack_depth(), 1);
        assert_eq!(menu.top().path, root);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn open_directory_pushes_and_restores() {
        let root = temp_root("open_dir");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "SFC");
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        let _guard = lock_tmp();

        let mut menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        // 打开 SFC 目录
        let launched = menu.open_directory(
            &format!("{root}/Roms/SFC"),
            false,
            &root,
            PLATFORM,
            &paks_path(&root, PLATFORM),
        );
        assert!(!launched);
        assert_eq!(menu.stack_depth(), 2);
        assert_eq!(menu.top().path, format!("{root}/Roms/SFC"));
        // end 初始 = min(entries, rows)
        assert_eq!(menu.top().end, 1);

        // 关闭 → restore 状态保存 → 重开恢复
        menu.close_directory();
        assert_eq!(menu.stack_depth(), 1);
        menu.top_mut().selected = 0; // 模拟根目录选择 SFC 条目
        let launched = menu.open_directory(
            &format!("{root}/Roms/SFC"),
            false,
            &root,
            PLATFORM,
            &paks_path(&root, PLATFORM),
        );
        assert!(!launched);
        assert_eq!(menu.stack_depth(), 2);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn open_directory_auto_launch_cue() {
        let root = temp_root("auto_cue");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "PS");
        let game = format!("{root}/Roms/PS/Final Fantasy VII");
        fs::create_dir_all(&game).unwrap();
        fs::write(format!("{game}/Final Fantasy VII.cue"), "x").unwrap();
        fs::write(format!("{game}/Track 1.bin"), "x").unwrap();
        let _guard = lock_tmp();

        let mut menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        let launched =
            menu.open_directory(&game, true, &root, PLATFORM, &paks_path(&root, PLATFORM));
        assert!(launched, "cue 命中应直接启动");
        assert_eq!(menu.stack_depth(), 1, "启动不入栈");
        // /tmp/next 已写入启动命令
        let next = common::utils::get_file("/tmp/next").unwrap_or_default();
        assert!(next.contains("launch.sh"), "应写入模拟器启动命令: {next}");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn open_directory_auto_launch_m3u() {
        let root = temp_root("auto_m3u");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "SFC");
        let game = format!("{root}/Roms/SFC/Final Fantasy VII");
        fs::create_dir_all(&game).unwrap();
        fs::write(format!("{game}/Final Fantasy VII.m3u"), "Disc 1.sfc\n").unwrap();
        fs::write(format!("{game}/Disc 1.sfc"), "x").unwrap();
        let _guard = lock_tmp();

        let mut menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        let launched =
            menu.open_directory(&game, true, &root, PLATFORM, &paks_path(&root, PLATFORM));
        assert!(launched, "m3u 命中应直接启动");
        assert_eq!(menu.stack_depth(), 1, "启动不入栈");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn open_directory_auto_launch_no_cue_m3u_pushes() {
        let root = temp_root("auto_none");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "SFC");
        let game = format!("{root}/Roms/SFC/Plain");
        fs::create_dir_all(&game).unwrap();
        fs::write(format!("{game}/A.sfc"), "x").unwrap();
        let _guard = lock_tmp();

        let mut menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        let launched =
            menu.open_directory(&game, true, &root, PLATFORM, &paks_path(&root, PLATFORM));
        assert!(!launched, "无 cue/m3u 应入栈");
        assert_eq!(menu.stack_depth(), 2);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn entry_open_rom_launches() {
        let root = temp_root("entry_rom");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "SFC");
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        let _guard = lock_tmp();

        let mut menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        // 进入 SFC 目录
        menu.open_directory(
            &format!("{root}/Roms/SFC"),
            false,
            &root,
            PLATFORM,
            &paks_path(&root, PLATFORM),
        );
        assert_eq!(menu.top().entries.len(), 1);

        let launched = menu.entry_open(0, &root, PLATFORM, &paks_path(&root, PLATFORM));
        assert!(launched);
        let next = common::utils::get_file("/tmp/next").unwrap_or_default();
        assert!(next.contains("Game.sfc"), "应写入 ROM 启动命令: {next}");
        // /tmp/last.txt 保存浏览位置
        let last = common::utils::get_file(crate::launch::LAST_PATH).unwrap_or_default();
        assert!(last.contains("Game.sfc"), "应保存浏览位置: {last}");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn entry_open_dir_descends() {
        let root = temp_root("entry_dir");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "SFC");
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        let _guard = lock_tmp();

        let mut menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        // 根目录选中 SFC 条目（index 0——无 recents 时第一个是 Roms 下 console）
        let idx = menu
            .top()
            .entries
            .iter()
            .position(|e| e.path == format!("{root}/Roms/SFC"))
            .expect("根目录应含 SFC");
        let launched = menu.entry_open(idx, &root, PLATFORM, &paks_path(&root, PLATFORM));
        assert!(!launched, "目录下钻不入栈不退出");
        assert_eq!(menu.stack_depth(), 2);
        assert_eq!(menu.top().path, format!("{root}/Roms/SFC"));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn entry_open_should_resume_consumed_once() {
        let root = temp_root("entry_resume");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "SFC");
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        let _guard = lock_tmp();

        let mut menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        menu.open_directory(
            &format!("{root}/Roms/SFC"),
            false,
            &root,
            PLATFORM,
            &paks_path(&root, PLATFORM),
        );
        menu.request_resume();
        menu.entry_open(0, &root, PLATFORM, &paks_path(&root, PLATFORM));
        assert!(!menu.should_resume, "should_resume 应被一次性消费");

        fs::remove_dir_all(&root).unwrap();
    }

    // ── load_last（spec 场景「load_last 恢复位置」）──

    #[test]
    fn load_last_restores_position() {
        let root = temp_root("load_last");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "SFC");
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        let _guard = lock_tmp();

        // last.txt 指向 ROM 文件路径（openRom 保存形态）→ 下钻到父目录并选中
        fs::write(
            crate::launch::LAST_PATH,
            format!("{root}/Roms/SFC/Game.sfc"),
        )
        .unwrap();

        let menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        assert_eq!(menu.stack_depth(), 2, "应下钻到 SFC");
        assert_eq!(menu.top().path, format!("{root}/Roms/SFC"));
        assert_eq!(menu.top().selected, 0, "ROM 条目被选中");
        assert_eq!(menu.top().entries[0].name, "Game");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn load_last_dir_path_stays_on_entry() {
        let root = temp_root("load_last_dir");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "SFC");
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        let _guard = lock_tmp();

        // last.txt 指向目录路径（openPak 保存形态）→ 不进入（C :1269），光标选中
        fs::write(crate::launch::LAST_PATH, format!("{root}/Roms/SFC")).unwrap();

        let menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        assert_eq!(menu.stack_depth(), 1, "目录路径不进入（停在其所在层）");
        let sfc_idx = menu
            .top()
            .entries
            .iter()
            .position(|e| e.path == format!("{root}/Roms/SFC"))
            .expect("根目录应含 SFC");
        assert_eq!(menu.top().selected, sfc_idx, "光标选中 SFC 条目");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn load_last_absent_keeps_root() {
        let root = temp_root("load_last_absent");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "SFC");
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        let _guard = lock_tmp();

        // 无 last.txt
        let menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        assert_eq!(menu.stack_depth(), 1, "无记录保持根目录");
        assert_eq!(menu.top().path, root);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn load_last_rom_path_selects_not_descends() {
        let root = temp_root("load_last_rom");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "SFC");
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        let _guard = lock_tmp();

        // last.txt 指向 ROM 文件本身（最后一层不显示内容）
        fs::write(
            crate::launch::LAST_PATH,
            format!("{root}/Roms/SFC/Game.sfc"),
        )
        .unwrap();

        let menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        assert_eq!(menu.stack_depth(), 2, "应下钻到 SFC 目录");
        assert_eq!(menu.top().selected, 0, "ROM 条目被选中");
        assert_eq!(menu.top().entries[0].name, "Game");

        fs::remove_dir_all(&root).unwrap();
    }

    // ── refresh_resume ──

    #[test]
    fn refresh_resume_slot_detection() {
        let root = temp_root("refresh_resume");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "SFC");
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        let _guard = lock_tmp();

        let mut menu = Menu::new(&root, PLATFORM, &paks_path(&root, PLATFORM), ROWS);
        menu.open_directory(
            &format!("{root}/Roms/SFC"),
            false,
            &root,
            PLATFORM,
            &paks_path(&root, PLATFORM),
        );
        menu.refresh_resume(&root);
        assert!(!menu.can_resume(), "无 slot 文件不可续玩");

        // 创建 slot 文件（{shared}/.minui/SFC/Game.sfc.txt）
        let slot = format!("{root}/.userdata/shared/.minui/SFC/Game.sfc.txt");
        fs::create_dir_all(std::path::Path::new(&slot).parent().unwrap()).unwrap();
        fs::write(&slot, "1").unwrap();
        menu.refresh_resume(&root);
        assert!(menu.can_resume(), "slot 存在可续玩");

        fs::remove_dir_all(&root).unwrap();
    }
}
