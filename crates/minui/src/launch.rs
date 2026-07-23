//! # 游戏启动逻辑
//!
//! 对应原 C 代码 `minui.c` 中处理游戏启动的函数。
//!
//! 这些函数负责从"选中一个条目"到"执行 minarch.elf"的整个流程。
//!
//! ## 核心流程
//!
//! ```text
//! 用户按 A 键
//!   → Entry_open()            分发：ROM/Pak/目录
//!     → openRom()             确定模拟器 → 构造 shell 命令
//!     → openPak()             直接执行 pak 的 launch.sh
//!     → openDirectory()       进入子目录（或自动启动 cue/m3u）
//!   → queueNext()             写入 /tmp/next，设置 quit=1
//!   → minui 退出
//!   → 外层 shell 读取 /tmp/next 并执行
//! ```
//!
//! ## 存档恢复
//!
//! 当 `should_resume==true`（用户按了 X）时：
//! 1. 读取存档槽位文件（包含存档槽位号）
//! 2. 对于多碟游戏，根据存档确定正确的碟号
//! 3. 将槽位号写入 `/tmp/resume_slot.txt`
//!
//! 普通启动（按 A）总是使用槽位 8（隐藏的默认存档）。

use std::fs;

use common::types::*;
use common::utils::*;
use common::paths;

use crate::state::MinUi;
use crate::scan;

// ============================================================================
// Shell 命令转义
// ============================================================================

/// 转义单引号以便安全嵌入 shell 命令 —— 对应 C 中的 `escapeSingleQuotes()`
///
/// 原理：在单引号字符串中，唯一不能直接出现的字符就是 `'` 自身。
/// 转义方式：结束当前单引号字符串 `'` → 添加转义的单引号 `\'` → 开始新的单引号字符串 `'`
///
/// 所以 `foo'bar` → `foo'\''bar`
///
/// ## 为什么需要这个
///
/// ROM 文件路径可能包含单引号（如 `Zelda's Adventure.gb`）。
/// minui 通过 `/tmp/next` 将命令传递给外层 shell 脚本，
/// 路径被包在单引号中以确保 shell 不会解释其中的特殊字符。
pub fn escape_single_quotes(s: &str) -> String {
    s.replace('\'', "'\\''")
}

/// 将命令写入 /tmp/next 并标记退出 —— 对应 C 中的 `queueNext()`
///
/// 这是 minui 与 minarch 之间的交接点：
/// - minui 写入命令到 `/tmp/next`
/// - 设置 `quit = true` 退出主循环
/// - 外层 shell 启动脚本读取 `/tmp/next`，执行下一阶段的命令
///
/// 为什么不用 `exec()` 直接启动 minarch？
/// 因为外层 shell 脚本需要在 minarch 退出后做清理工作（同步存档等）。
pub fn queue_next(state: &mut MinUi, cmd: &str) {
    log::info!("queue_next: {}", cmd);
    let _ = put_file(paths::NEXT_CMD_PATH, cmd);
    state.request_quit();
}

// ============================================================================
// 存档恢复检测
// ============================================================================

/// 检测指定 ROM 是否有可恢复的存档 —— 对应 C 中的 `readyResumePath()`
///
/// 存档位置：`<SHARED_USERDATA>/.minui/<EMU_TAG>/<ROM_FILENAME>.txt`
///
/// 对于多碟游戏（m3u），使用 m3u 文件名而非单个碟文件名。
/// 设置 `state.can_resume` 和 `state.slot_path`。
pub fn ready_resume_path(
    state: &mut MinUi,
    rom_path: &str,
    entry_type: EntryType,
    sdcard: &str,
    _platform_tag: &str,
) {
    state.can_resume = false;

    let roms = scan::roms_path(sdcard);

    // 只有 ROMS_PATH 下的路径才能有存档恢复
    if !prefix_match(&roms, rom_path) {
        return;
    }

    let mut path = rom_path.to_string();

    // 对于目录类型的条目（PS1 等），查找 cue 或 m3u
    if entry_type == EntryType::Dir {
        if let Some(cue) = scan::find_cue(&path) {
            path = cue;
        } else {
            // 尝试查找 m3u
            let parent_name = file_name(&path).unwrap_or("");
            let grandparent = parent_dir(&path).unwrap_or("");
            let m3u = format!("{}/{}.m3u", grandparent, parent_name);
            if path_exists(&m3u) {
                path = m3u;
            } else {
                return; // 没有 cue 也没有 m3u
            }
        }
    }

    // 对于非 m3u 路径，检查是否有对应的 m3u
    if !is_m3u(&path) {
        if let Some(m3u) = scan::find_m3u(&path) {
            path = m3u;
        }
    }

    // 提取模拟器标签和 ROM 文件名
    let emu_name = get_emu_name(&path, &roms);
    let rom_file = file_name(&path).unwrap_or(&path);

    // 构造存档路径
    let slot_full = paths::slot_path_direct(sdcard, &emu_name, rom_file);

    state.can_resume = path_exists(&slot_full);
    state.slot_path = slot_full;
}

/// 检测 Entry 是否有可恢复的存档 —— 对应 C 中的 `readyResume()`
pub fn ready_resume(
    state: &mut MinUi,
    entry: &Entry,
    sdcard: &str,
    _platform_tag: &str,
) {
    ready_resume_path(state, &entry.path, entry.entry_type, sdcard, _platform_tag);
}

// ============================================================================
// 自动恢复（非正常关机后）
// ============================================================================

/// 自动恢复上次游戏 —— 对应 C 中的 `autoResume()`
///
/// 如果 `/.userdata/shared/.minui/auto_resume.txt` 文件存在：
/// 1. 读取文件内容（ROM 的相对路径）
/// 2. 构造完整路径，验证 ROM 和模拟器仍然存在
/// 3. 使用存档槽位 9 直接启动游戏
/// 4. 删除 auto_resume.txt
///
/// 这个文件在 minarch 异常退出时（如设备没电）写入。
/// 下次开机时 minui 检测到这个文件，直接恢复游戏，用户感觉不到中断。
///
/// 返回 `true` 表示已处理自动恢复（调用方应直接退出，不显示界面）。
pub fn auto_resume(
    sdcard: &str,
    platform_tag: &str,
    paks: &str,
) -> bool {
    let auto_path = paths::auto_resume_path_direct(sdcard);

    if !path_exists(&auto_path) {
        return false;
    }

    // 读取 ROM 相对路径
    let relative = match fs::read_to_string(&auto_path) {
        Ok(s) => s.trim().to_string(),
        Err(_) => return false,
    };

    // 删除文件（只处理一次）
    let _ = fs::remove_file(&auto_path);

    // 构造完整路径
    let sd_path = format!("{}/{}", sdcard, relative);
    if !path_exists(&sd_path) {
        return false;
    }

    // 验证模拟器仍然存在
    let roms = scan::roms_path(sdcard);
    let emu_name = get_emu_name(&sd_path, &roms);

    let emu_path = scan::get_emu_path(&emu_name, sdcard, platform_tag, paks);
    if !path_exists(&emu_path) {
        return false;
    }

    // 写入存档槽位 9（自动恢复专用）
    let _ = put_int(paths::RESUME_SLOT_PATH, scan::AUTO_RESUME_SLOT);

    // 构造并写入启动命令
    let cmd = format!(
        "'{}' '{}'",
        escape_single_quotes(&emu_path),
        escape_single_quotes(&sd_path)
    );
    let _ = put_file(paths::NEXT_CMD_PATH, &cmd);

    true
}

// ============================================================================
// 打开 Pak（工具/模拟器包）
// ============================================================================

/// 打开一个 Pak 条目 —— 对应 C 中的 `openPak()`
///
/// Pak 是一个包含 `launch.sh` 的目录。启动 Pak 就是执行其 launch.sh。
///
/// 如果是 ROMS_PATH 下的 Pak（模拟器游戏），加入最近游戏。
pub fn open_pak(
    state: &mut MinUi,
    path: &str,
    sdcard: &str,
    roms: &str,
) {
    // ROMS_PATH 下的 Pak → 加入最近游戏
    if prefix_match(roms, path) {
        state.add_recent_direct(path, None, sdcard);
    }

    let escaped = escape_single_quotes(path);
    let cmd = format!("'{}/launch.sh'", escaped);
    queue_next(state, &cmd);
}

// ============================================================================
// 打开 ROM（启动游戏）
// ============================================================================

/// 打开一个 ROM 条目 —— 对应 C 中的 `openRom()`
///
/// 这是 MinUI 中最核心的函数之一。处理完整的游戏启动流程。
///
/// ## 流程
///
/// 1. **多碟处理**：如果 ROM 有 m3u 文件，使用 m3u 路径
/// 2. **存档恢复**：如果 `should_resume`：
///    - 从存档槽位文件读取槽位号
///    - 如果是多碟游戏，根据存档找到正确的碟号
///    - 写入 `/tmp/resume_slot.txt`
/// 3. **普通启动**：写入默认槽位 8
/// 4. **找到模拟器路径** → 构造 shell 命令 → queue_next
///
/// ## 参数
///
/// - `path`: ROM 的完整 SD 卡路径
/// - `last`: 可选，"最后位置"路径。用于 Collections 中时记录 collection 文件路径
/// - `alias`: 可选的显示别名（加入最近游戏时使用）
pub fn open_rom(
    state: &mut MinUi,
    path: &str,
    last: Option<&str>,
    alias: Option<&str>,
    sdcard: &str,
    platform_tag: &str,
    paks: &str,
) {
    log::info!("open_rom({}, {:?})", path, last);

    let mut sd_path = path.to_string();

    // 1. 多碟游戏处理
    let m3u = scan::find_m3u(&sd_path);
    let has_m3u = m3u.is_some();

    // 用于记录"最近游戏"的路径 —— 用 m3u 路径（如果有的话）
    let recent_path = m3u.clone().unwrap_or_else(|| sd_path.clone());

    // 如果 sd_path 本身是 m3u，取第一个碟作为实际路径
    if let Some(ref m3u_path) = m3u {
        if is_m3u(&sd_path) {
            if let Some(first_disc) = scan::get_first_disc(m3u_path) {
                sd_path = first_disc;
            }
        }
    }

    // 确定模拟器名称
    let roms = scan::roms_path(sdcard);
    let emu_name = get_emu_name(&sd_path, &roms);

    // 2. 存档恢复（X 键）
    if state.should_resume && !state.slot_path.is_empty() {
        // 从存档槽位文件读取槽位号
        if let Ok(slot_str) = std::fs::read_to_string(&state.slot_path) {
            let slot = slot_str.trim().to_string();
            if !slot.is_empty() {
                let _ = put_file(paths::RESUME_SLOT_PATH, &slot);
            }
        }
        state.should_resume = false;

        // 多碟游戏：根据存档确定正确的碟号
        if has_m3u {
            if let Some(ref m3u_path) = m3u {
                let rom_file = file_name(m3u_path).unwrap_or("");
                let disc_slot = paths::disc_slot_path_direct(
                    sdcard, &emu_name, rom_file, &state.slot_path,
                );

                if path_exists(&disc_slot) {
                    if let Ok(disc_path) = std::fs::read_to_string(&disc_slot) {
                        let disc_path = disc_path.trim();
                        if !disc_path.is_empty() {
                            if disc_path.starts_with('/') {
                                // 绝对路径
                                sd_path = disc_path.to_string();
                            } else {
                                // 相对路径（相对于 m3u 文件目录）
                                let m3u_dir = parent_dir(m3u_path).unwrap_or("");
                                sd_path = format!("{}/{}", m3u_dir, disc_path);
                            }
                        }
                    }
                }
            }
        }
    } else {
        // 普通启动（A 键）：使用默认存档槽位 8
        let _ = put_int(paths::RESUME_SLOT_PATH, scan::DEFAULT_SLOT);
    }

    // 3. 找到模拟器路径
    let emu_path = scan::get_emu_path(&emu_name, sdcard, platform_tag, paks);

    // 4. 加入最近游戏
    state.add_recent_direct(&recent_path, alias, sdcard);

    // 5. 构造并写入命令
    let cmd = format!(
        "'{}' '{}'",
        escape_single_quotes(&emu_path),
        escape_single_quotes(&sd_path)
    );
    queue_next(state, &cmd);
}

// ============================================================================
// Entry 打开分发
// ============================================================================

/// 打开一个 Entry —— 对应 C 中的 `Entry_open()`
///
/// 根据条目类型分发到不同的处理逻辑：
/// - `Rom` → `open_rom()`
/// - `Pak` → `open_pak()`
/// - `Dir` → 由调用者调用 `open_directory()`
///
/// 同时处理 Collections 中的 last 路径逻辑：
/// 在 Collections 中启动的 ROM，其 "最后位置" 应指向 collection 文件，
/// 这样返回时能回到正确的 collection。
pub fn entry_open(
    state: &mut MinUi,
    entry: &Entry,
    sdcard: &str,
    platform_tag: &str,
    paks: &str,
) {
    let roms = scan::roms_path(sdcard);

    match entry.entry_type {
        EntryType::Rom => {
            // 计算 last 路径（用于 Collections）
            let last = if state.is_in_collection(sdcard) {
                // 将 ROM 文件名拼接到 collection 文件路径
                let filename = file_name(&entry.path).unwrap_or("");
                let current_path = &state.current_dir().path;
                Some(format!("{}/{}", current_path, filename))
            } else {
                None
            };
            let alias = Some(entry.name.as_str());
            open_rom(state, &entry.path, last.as_deref(), alias, sdcard, platform_tag, paks);
        }
        EntryType::Pak => {
            open_pak(state, &entry.path, sdcard, &roms);
        }
        EntryType::Dir => {
            // auto_launch = true：如果目录下有 cue/m3u 则自动启动
            state.open_directory(&entry.path, true, sdcard, platform_tag, paks);
            state.mark_dirty();
        }
    }
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_single_quotes_basic() {
        assert_eq!(escape_single_quotes("hello"), "hello");
        assert_eq!(
            escape_single_quotes("Zelda's Adventure"),
            "Zelda'\\''s Adventure"
        );
        assert_eq!(
            escape_single_quotes("it's o'clock"),
            "it'\\''s o'\\''clock"
        );
    }

    #[test]
    fn test_escape_single_quotes_path() {
        let path = "/mnt/sdcard/Roms/GB/Zelda's Quest (World).gb";
        let escaped = escape_single_quotes(path);
        // 转义后的字符串中每个 ' 都被替换为 '\''，
        // 所以总字符数比原始字符串长（每个 ' 多 3 个字符）
        assert!(escaped.len() > path.len());
        // 包含转义模式
        assert!(escaped.contains("\\'"));
        // 已被转义：原始的单引号已经被替换
        assert_eq!(escaped, "/mnt/sdcard/Roms/GB/Zelda'\\''s Quest (World).gb");
    }
}
