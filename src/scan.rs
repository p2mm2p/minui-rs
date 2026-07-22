//! # 文件系统扫描
//!
//! 对应原 C 代码 `minui.c` 中的静态扫描函数（`getRoot`, `getEntries`, `getRecents` 等）。
//!
//! 这些函数负责遍历 SD 卡上的目录结构，构建 `Entry` 和 `Directory` 对象。
//!
//! ## 设计原则
//!
//! 所有函数都是**纯函数** —— 接收路径参数，返回构建好的数据结构。
//! 不依赖全局状态（除了已传入的 `recents` 列表）。
//! 这使得函数可独立测试，不需要实际硬件。

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::types::*;
use crate::utils::*;

// ============================================================================
// 常量
// ============================================================================

/// 自动恢复使用的存档槽位号 —— 对应 C 中的 `AUTO_RESUME_SLOT 9`
pub const AUTO_RESUME_SLOT: i32 = 9;

/// 普通启动使用的默认存档槽位号 —— 对应 C 中 `putInt(RESUME_SLOT_PATH, 8)`
pub const DEFAULT_SLOT: i32 = 8;

/// 最大最近游戏条目数 —— 对应 C 中的 `MAX_RECENTS 24`
pub const MAX_RECENTS: usize = 24;

// ============================================================================
// 路径构造辅助
// ============================================================================

/// ROM 目录路径 —— `{SDCARD}/Roms`
///
/// # 调用者
/// `get_root`, `has_roms`, `get_entries`, `is_console_dir`, `load_recents`
fn roms_path(sdcard: &str) -> String {
    format!("{}/Roms", sdcard)
}

/// 最近游戏伪目录路径 —— `{SDCARD}/Recently Played`
/// 注意：这不是一个真实的文件系统目录，而是由 `get_recents_from_list` 动态生成的
///
/// # 调用者
/// `get_root`（判断是否显示这一项）, `make_directory`（识别伪目录）
fn faux_recent_path(sdcard: &str) -> String {
    format!("{}/Recently Played", sdcard)
}

/// 收藏夹目录路径 —— `{SDCARD}/Collections`
///
/// # 调用者
/// `get_root`（判断是否显示/提升 Collections）, `make_directory`（识别伪目录）
fn collections_path(sdcard: &str) -> String {
    format!("{}/Collections", sdcard)
}

/// 系统 Pak 目录路径 —— `{SDCARD}/.system/{PLATFORM}/paks`
///
/// # 调用者
/// 被 `has_emu` 用于查找系统内置的模拟器 Pak（路径1）
#[allow(dead_code)]
fn paks_path(sdcard: &str, platform: &str) -> String {
    format!("{}/.system/{}/paks", sdcard, platform)
}

/// 共享用户数据目录路径 —— `{SDCARD}/.userdata/shared`
///
/// # 调用者
/// `recent_file_path`（最近游戏文件在此目录下）, `paths::slot_path`（存档状态在此目录下）
fn shared_userdata_path(sdcard: &str) -> String {
    format!("{}/.userdata/shared", sdcard)
}

/// 最近游戏记录文件路径 —— `{SHARED_USERDATA}/.minui/recent.txt`
///
/// # 调用者
/// `load_recents` → 启动时从此文件恢复最近游戏历史
fn recent_file_path(sdcard: &str) -> String {
    format!("{}/.minui/recent.txt", shared_userdata_path(sdcard))
}

// ============================================================================
// Entry 构造
// ============================================================================

/// 创建一个新的 Entry —— 对应 C 中的 `Entry_new()`
///
/// 自动调用 `get_display_name` 提取显示名。
///
/// # 调用者
/// `scan_dir`, `get_root`, `get_recents_from_list`, `get_collection`, `get_discs`
/// —— 所有需要构造 Entry 的函数都经过这个工厂函数
fn create_entry(path: &str, entry_type: EntryType) -> Entry {
    let name = get_display_name(path);
    Entry {
        path: path.to_string(),
        name,
        unique: None,
        entry_type,
        alpha: 0,
    }
}

/// 创建带平台标签处理的 Entry（用于 Tools 目录）
#[allow(dead_code)]
fn create_entry_with_platform(path: &str, entry_type: EntryType, platform_tag: &str) -> Entry {
    let name = get_display_name_with_platform(path, platform_tag);
    Entry {
        path: path.to_string(),
        name,
        unique: None,
        entry_type,
        alpha: 0,
    }
}

// ============================================================================
// 模拟器可用性检查
// ============================================================================

/// 检查模拟器是否可用 —— 对应 C 中的 `hasEmu()`
///
/// 查找两个位置：
/// 1. `<paks_path>/Emus/<emu_name>.pak/launch.sh` — 系统内置 Pak
/// 2. `<sdcard>/Emus/<platform>/<emu_name>.pak/launch.sh` — 平台专属 Pak
///
/// ### 调用者
///
/// | 调用者 | 效果 |
/// |--------|------|
/// | `has_roms` | 扫描 ROMS 时判断该主机是否有可用模拟器 |
/// | `load_recents` | 载入最近游戏时判断该游戏的模拟器当前是否仍可用（设置 `Recent.available`） |
/// | `open_rom`（未来） | 启动游戏前最后确认模拟器存在 |
pub fn has_emu(
    emu_name: &str,
    sdcard_path: &str,
    platform_tag: &str,
    paks: &str,
) -> bool {
    // 位置 1：系统 Pak
    let pak1 = format!("{}/Emus/{}.pak/launch.sh", paks, emu_name);
    if path_exists(&pak1) {
        return true;
    }

    // 位置 2：SD 卡平台专属
    let pak2 = format!(
        "{}/Emus/{}/{}.pak/launch.sh",
        sdcard_path, platform_tag, emu_name
    );
    path_exists(&pak2)
}

/// 检查目录中是否有 CUE 文件 —— 对应 C 中的 `hasCue()`
///
/// PS1 游戏通常是一个目录 + 一个同名的 .cue 文件。
/// 例如：`/Roms/PS/Game/` → 检查 `/Roms/PS/Game/Game.cue`
///
/// 返回 `Some(cue_path)` 如果找到，否则 `None`
///
/// # 调用者
/// `open_directory`（未来） → 用户进入 PS1 游戏目录时，如果发现 cue 文件且 `auto_launch=true`，
/// 则跳过目录浏览，直接启动该 cue 文件对应的游戏。
pub fn find_cue(dir_path: &str) -> Option<String> {
    let dir_name = Path::new(dir_path)
        .file_name()
        .and_then(|n| n.to_str())?;

    let cue_path = format!("{}/{}.cue", dir_path, dir_name);
    if path_exists(&cue_path) {
        Some(cue_path)
    } else {
        None
    }
}

/// 检查 ROM 是否有对应的 M3U 文件 —— 对应 C 中的 `hasM3u()`
///
/// M3U 文件的位置：ROM 所在目录的上级目录中，文件名为上级目录名 + `.m3u`。
///
/// ```text
/// ROM:   /sdcard/Roms/PS/Final Fantasy VII/Disc 1.cue
/// 父目录: /sdcard/Roms/PS/Final Fantasy VII/
/// 上上级: /sdcard/Roms/PS/
/// M3U:   /sdcard/Roms/PS/Final Fantasy VII.m3u
/// ```
///
/// 返回 `Some(m3u_path)` 如果找到，否则 `None`
///
/// ### 调用者
///
/// | 调用者 | 效果 |
/// |--------|------|
/// | `load_recents` | 多碟游戏去重：同一 m3u 下的多个碟只保留一个最近条目 |
/// | `open_rom`（未来） | 启动单碟 ROM 时检测它是否属于多碟游戏，若是则用 m3u 路径替代 |
/// | `open_directory`（未来） | 检测 PS1 目录是否有多碟 m3u，若有则自动启动 |
pub fn find_m3u(rom_path: &str) -> Option<String> {
    // 获取 ROM 的父目录
    let rom_parent = Path::new(rom_path).parent()?.to_str()?;

    // 获取父目录的名字
    let parent_name = Path::new(rom_parent)
        .file_name()
        .and_then(|n| n.to_str())?;

    // 获取上上级目录
    let grandparent = Path::new(rom_parent).parent()?.to_str()?;

    // 构造 M3U 路径：上上级目录 / 父目录名.m3u
    let m3u_path = format!("{}/{}.m3u", grandparent, parent_name);

    if path_exists(&m3u_path) {
        Some(m3u_path)
    } else {
        None
    }
}

/// 检查顶层游戏主机目录下是否有 ROM —— 对应 C 中的 `hasRoms()`
///
/// 条件：
/// 1. 有对应的模拟器 Pak
/// 2. 目录下至少有一个非隐藏文件
///
/// # 调用者
/// `get_root` → 遍历 ROMS 目录时决定哪些游戏主机出现在主菜单中。
/// 无 ROM 或无模拟器的目录不出现在用户面前。
pub fn has_roms(
    dir_name: &str,
    sdcard_path: &str,
    platform_tag: &str,
    paks: &str,
) -> bool {
    let emu_name = get_emu_name(dir_name, &roms_path(sdcard_path));
    if !has_emu(&emu_name, sdcard_path, platform_tag, paks) {
        return false;
    }

    let rom_dir = format!("{}/{}", roms_path(sdcard_path), dir_name);
    scan_dir_has_visible(&rom_dir)
}

// ============================================================================
// 目录扫描辅助
// ============================================================================

/// 扫描目录中是否存在非隐藏条目
///
/// # 调用者
/// `has_roms` → 双重检查的第二关：模拟器存在后，确认目录下确实有 ROM 文件（非空）
/// `get_root` → 检查 Collections 目录是否有内容，决定是否显示/提升 Collections
fn scan_dir_has_visible(path: &str) -> bool {
    match fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries.filter_map(|e| e.ok()) {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if !hide(&name_str) {
                    return true;
                }
            }
            false
        }
        Err(_) => false,
    }
}

/// 判断一个路径是否是顶层游戏主机目录（父目录是 ROMS_PATH）
///
/// # 调用者
/// `get_entries` → 决定是否执行归类（collation）逻辑：只有顶层主机目录才需要合并同系列变体
fn is_console_dir(path: &str, sdcard: &str) -> bool {
    if let Some(parent) = parent_dir(path) {
        return parent == roms_path(sdcard);
    }
    false
}

// ============================================================================
// 主扫描函数
// ============================================================================

/// 扫描目录并添加条目 —— 对应 C 中的 `addEntries()`
///
/// 遍历目录中的所有条目，过滤隐藏文件，按类型创建 Entry。
///
/// 类型判定规则：
/// - `.pak` 目录 → `EntryType::Pak`
/// - 普通目录 → `EntryType::Dir`
/// - Collections 下的文本文件 → `EntryType::Dir`（伪装成目录）
/// - 其他文件 → `EntryType::Rom`
///
/// ### 调用者
///
/// | 调用者 | 效果 |
/// |--------|------|
/// | `get_entries` | 扫描普通子目录中的 ROM 和 Pak |
/// | `get_root` | 当 Collections 被提升到根目录时扫描收藏文件 |
/// | `get_collection` | （间接通过 `getEntries` 被调用） |
pub fn scan_dir(path: &str, is_collection: bool) -> Vec<Entry> {
    let mut entries = Vec::new();

    let dir_iter = match fs::read_dir(path) {
        Ok(iter) => iter,
        Err(_) => return entries,
    };

    for entry in dir_iter.filter_map(|e| e.ok()) {
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();

        if hide(&name_str) {
            continue;
        }

        let full_path = entry.path();
        let full_path_str = full_path.to_string_lossy().to_string();

        let entry_type = if full_path.is_dir() {
            if is_pak(&name_str) {
                EntryType::Pak
            } else {
                EntryType::Dir
            }
        } else if is_collection {
            // Collections 下的 .txt 文件伪装成目录
            EntryType::Dir
        } else {
            EntryType::Rom
        };

        entries.push(create_entry(&full_path_str, entry_type));
    }

    entries
}

/// 获取目录内容 —— 对应 C 中的 `getEntries()`
///
/// 如果是顶层游戏主机目录，执行**归类**（collation）：
/// 将共享相同前缀括号的目录合并在一起。例如 "Game Boy (GB)" 和 "Game Boy (GBC)"
/// 会归入同一个列表，因为它们都以 "Game Boy" 开头。
///
/// 如果不是顶层目录，直接 `scan_dir`。
///
/// # 调用者
/// `make_directory` → 当用户进入一个目录时调用。`make_directory` 判断路径类型后委托给此函数。
/// 此函数是用户看到 ROM 列表的**核心数据源**。
pub fn get_entries(path: &str, sdcard: &str) -> Vec<Entry> {
    let mut entries = Vec::new();

    if is_console_dir(path, sdcard) {
        // 顶层游戏主机目录 —— 可能需要归类
        let collated_prefix = extract_collate_prefix(path);

        let roms = roms_path(sdcard);
        if let Ok(dir_iter) = fs::read_dir(&roms) {
            for entry in dir_iter.filter_map(|e| e.ok()) {
                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();

                if hide(&name_str) {
                    continue;
                }

                let full_path = entry.path();
                let full_path_str = full_path.to_string_lossy().to_string();

                if !full_path.is_dir() {
                    continue;
                }

                // 检查是否匹配归类前缀
                if !prefix_match(&collated_prefix, &full_path_str) {
                    continue;
                }

                entries.append(&mut scan_dir(&full_path_str, false));
            }
        }
    } else {
        // 普通子目录
        entries = scan_dir(path, false);
    }

    // 排序（大小写不敏感）
    entries.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });

    entries
}

/// 从路径提取归类前缀 —— 对应 C 中 getEntries 的 collated_path 逻辑
///
/// "Game Boy (GB)" → "Game Boy ("
/// 保留开头括号是为了避免 "Game Boy" 同时匹配 "Game Boy Advance" 和 "Game Boy Color"
///
/// # 调用者
/// `get_entries` → 在归类模式下，用提取的前缀去匹配 ROMS 下的其他目录，
/// 把同系列不同变体的目录合并到同一个列表中
fn extract_collate_prefix(path: &str) -> String {
    // 找到最后一个 '('
    if let Some(paren_pos) = path.rfind('(') {
        // 保留到 '(' 之后一个字符，即 '(' 本身也被保留
        // 格式：/sdcard/Roms/Game Boy (GB)
        //       → /sdcard/Roms/Game Boy (
        path[..=paren_pos].to_string()
    } else {
        // 没有括号 → 精确匹配
        path.to_string()
    }
}

// ============================================================================
// 特殊目录扫描
// ============================================================================

/// 构建根目录 —— 对应 C 中的 `getRoot()`
///
/// 根目录包含：
/// 1. "Recently Played"（如果有最近游戏记录）
/// 2. 所有有 ROM 的游戏主机目录
/// 3. "Collections"（如果有收藏）
/// 4. "Tools"（如果存在且不是 simple_mode）
///
/// ## 名称映射
///
/// 读取 `<ROMS>/map.txt` 文件来重命名条目。
/// 格式：`原始目录名\t别名\n`
///
/// ## Collections 提升
///
/// 如果没有任何可见的游戏系统，Collections 会被直接提升到根目录。
///
/// # 调用者
/// `Menu_init`（未来） → 启动时调用一次，构建用户看到的主屏幕。
/// 这是整个 launcher 的**入口数据源**。
pub fn get_root(
    sdcard: &str,
    platform_tag: &str,
    paks: &str,
    has_any_recent: bool,
    _has_any_collections: bool,
    simple_mode: bool,
) -> Vec<Entry> {
    let mut root: Vec<Entry> = Vec::new();
    let roms = roms_path(sdcard);

    // 1. 最近游戏
    if has_any_recent {
        root.push(create_entry(&faux_recent_path(sdcard), EntryType::Dir));
    }

    // 2. 扫描游戏主机目录
    let mut entries: Vec<Entry> = Vec::new();
    let mut emus: Vec<Entry> = Vec::new();

    if let Ok(dir_iter) = fs::read_dir(&roms) {
        for entry in dir_iter.filter_map(|e| e.ok()) {
            let name = entry.file_name();
            let name_str = name.to_string_lossy().to_string();

            if hide(&name_str) {
                continue;
            }

            let full_path = entry.path();
            let full_path_str = full_path.to_string_lossy().to_string();

            if !full_path.is_dir() {
                continue;
            }

            if has_roms(&name_str, sdcard, platform_tag, paks) {
                emus.push(create_entry(&full_path_str, EntryType::Dir));
            }
        }
    }

    // 排序并去重（同名条目只保留第一个）
    emus.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });

    let mut prev_name: Option<String> = None;
    for entry in emus {
        let should_keep = match &prev_name {
            Some(prev) => entry.name != *prev,
            None => true,
        };
        if should_keep {
            prev_name = Some(entry.name.clone());
            entries.push(entry);
        }
    }

    // 3. 应用 map.txt 名称映射
    let map_path = format!("{}/map.txt", roms);
    if !entries.is_empty() && path_exists(&map_path) {
        apply_name_map(&mut entries, &map_path);
    }

    // 4. Collections
    let collections = collections_path(sdcard);
    let has_collections = path_exists(&collections) && scan_dir_has_visible(&collections);

    if has_collections {
        if !entries.is_empty() {
            root.push(create_entry(&collections, EntryType::Dir));
        } else {
            // 没有可见系统 → 把 Collections 的内容直接提升到根目录
            let mut coll_entries = scan_dir(&collections, true);
            coll_entries.sort_by(|a, b| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            });
            entries.append(&mut coll_entries);
        }
    }

    // 5. 将系统添加到 root
    root.append(&mut entries);

    // 6. Tools
    if !simple_mode {
        let tools_path = format!("{}/Tools/{}", sdcard, platform_tag);
        if path_exists(&tools_path) {
            root.push(create_entry(&tools_path, EntryType::Dir));
        }
    }

    root
}

/// 构建最近游戏条目列表 —— 对应 C 中的 `getRecents()`
///
/// 从 `recents`（最近游戏数据）转换为 `Entry` 列表（用于 UI 显示）。
/// 跳过不可用的条目（模拟器不在当前设备上）。
///
/// # 调用者
/// `make_directory` → 用户进入 "Recently Played" 伪目录时，将 `MinUi.recents` 转化为可显示的 Entry 列表
pub fn get_recents_from_list(
    recents: &[Recent],
    sdcard: &str,
) -> Vec<Entry> {
    recents
        .iter()
        .filter(|r| r.available)
        .map(|r| {
            let sd_path = format!("{}{}", sdcard, r.path);
            let entry_type = if is_pak(&sd_path) {
                EntryType::Pak
            } else {
                EntryType::Rom
            };
            let mut entry = create_entry(&sd_path, entry_type);
            if let Some(ref alias) = r.alias {
                entry.name = alias.clone();
            }
            entry
        })
        .collect()
}

/// 构建收藏列表条目 —— 对应 C 中的 `getCollection()`
///
/// 读取 `.txt` 文件，每行是一个相对路径（相对于 SDCARD_PATH）。
/// 检查路径是否存在，创建对应的 Entry。
///
/// # 调用者
/// `make_directory` → 用户进入某个 Collection 文件时（`.txt` 伪装成目录），读取该文件内容并构建条目列表
pub fn get_collection(collection_path: &str, sdcard: &str) -> Vec<Entry> {
    let mut entries = Vec::new();

    let content = match std::fs::read_to_string(collection_path) {
        Ok(c) => c,
        Err(_) => return entries,
    };

    for line in content.lines() {
        let line = normalize_newline(line);
        let line = trim_trailing_newlines(&line).trim();
        if line.is_empty() {
            continue;
        }

        let sd_path = format!("{}/{}", sdcard, line);
        if path_exists(&sd_path) {
            let entry_type = if is_pak(&sd_path) {
                EntryType::Pak
            } else {
                EntryType::Rom
            };
            entries.push(create_entry(&sd_path, entry_type));
        }
    }

    entries
}

/// 构建多碟游戏条目列表 —— 对应 C 中的 `getDiscs()`
///
/// 读取 `.m3u` 文件，每行是一个碟的路径（相对于 m3u 文件所在目录）。
/// 为每个存在的碟创建名为 "Disc N" 的 Entry。
///
/// # 调用者
/// `make_directory` → 用户进入一个 m3u 文件时（视为多碟游戏的子目录），展示所有碟供选择
pub fn get_discs(m3u_path: &str) -> Vec<Entry> {
    let mut entries = Vec::new();

    // 获取 m3u 文件所在目录
    let base_path = match Path::new(m3u_path).parent().and_then(|p| p.to_str()) {
        Some(p) => p,
        None => return entries,
    };

    let content = match std::fs::read_to_string(m3u_path) {
        Ok(c) => c,
        Err(_) => return entries,
    };

    let mut disc = 0usize;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let disc_path = format!("{}/{}", base_path, line);
        if path_exists(&disc_path) {
            disc += 1;
            let mut entry = create_entry(&disc_path, EntryType::Rom);
            entry.name = format!("Disc {}", disc);
            entries.push(entry);
        }
    }

    entries
}

/// 获取多碟游戏的第一张碟 —— 对应 C 中的 `getFirstDisc()`
///
/// 解析 m3u 文件，返回第一个有效碟的完整路径。
///
/// # 调用者
/// `open_rom`（未来） → 用户选择 m3u 文件本身时，获取第一张碟作为默认启动目标
/// `open_directory`（未来） → 目录自动启动时发现 m3u，获取第一张碟
pub fn get_first_disc(m3u_path: &str) -> Option<String> {
    let base_path = Path::new(m3u_path).parent()?.to_str()?;

    let content = std::fs::read_to_string(m3u_path).ok()?;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let disc_path = format!("{}/{}", base_path, line);
        if path_exists(&disc_path) {
            return Some(disc_path);
        }
    }

    None
}

// ============================================================================
// Directory 构造与索引
// ============================================================================

/// 创建 Directory 并建立字母索引 —— 对应 C 中的 `Directory_new()` + `Directory_index()`
///
/// ## 字母索引（alphas）
///
/// 字母索引数组使得 L1/R1 可以快速跳转到不同首字母的条目。
/// `alphas[i]` 存储的是第 i 个字母分组在 entries 中的起始索引。
///
/// ## 同名条目处理
///
/// 当两个相邻条目有完全相同的显示名时：
/// - 如果文件名也相同 → 构造 `"Name (EMU_TAG)"` 作为 unique
/// - 如果文件名不同 → 用各自的文件名作为 unique
///
/// ## map.txt 映射
///
/// 如果目录下有 `map.txt` 文件，会在索引前应用名称映射。
/// 映射后的条目如果 `hide()` 返回 true，会被过滤掉。
///
/// ### 调用者
///
/// `make_directory` 是整个导航系统的**唯一入口**。每次用户进入一个目录都会调用它：
///
/// | 路径类型 | 调用的数据源 | 效果 |
/// |----------|-------------|------|
/// | `SDCARD_PATH`（根） | `get_root` | 构建主屏幕 |
/// | `FAUX_RECENT_PATH`（最近游戏伪目录） | `get_recents_from_list` | 显示最近游戏 |
/// | Collection `.txt` 文件 | `get_collection` | 显示收藏列表 |
/// | `.m3u` 文件（多碟游戏） | `get_discs` | 显示碟号选择 |
/// | 普通目录 / 游戏主机目录 | `get_entries` | 显示 ROM 列表 |
///
/// 调用方：`open_directory`（未来）→ 用户导航到新目录时调用
pub fn make_directory(
    path: &str,
    entries: Vec<Entry>,
    selected: usize,
    sdcard: &str,
    platform_tag: &str,
) -> Directory {
    let name = get_display_name(path);
    let mut entries = entries;

    // 应用 map.txt（如果在 Roms 目录结构中）
    // map.txt 通常在路径本身或上级目录
    apply_directory_maps(&mut entries, path, sdcard);

    // 判断是否需要跳过字母索引
    let is_faux_recent = path == faux_recent_path(sdcard);
    let is_collection = prefix_match(&collections_path(sdcard), path)
        && !exact_match(path, &collections_path(sdcard));
    let skip_index = is_faux_recent || is_collection;

    // 处理同名条目和字母索引
    let mut alphas: Vec<usize> = Vec::new();
    let mut alpha_index: isize = -1;

    // 先排序
    entries.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });

    for i in 0..entries.len() {
        // 同名处理（与后一条比较）
        if i + 1 < entries.len() {
            let current = &entries[i];
            let next = &entries[i + 1];

            if current.name == next.name {
                // 两个条目显示名相同 → 需要 unique 字段区分
                let current_filename = file_name(&current.path);
                let next_filename = file_name(&next.path);

                let (_current_unique, _next_unique) = if current_filename == next_filename {
                    // 同名文件夹（如两个 "Game Boy" 主机目录）
                    let u1 = make_unique_name(&current.path, platform_tag);
                    let u2 = make_unique_name(&next.path, platform_tag);
                    (Some(u1), Some(u2))
                } else {
                    // 同目录下同名文件（不同版本）
                    (
                        current_filename.map(String::from),
                        next_filename.map(String::from),
                    )
                };

                // 需要通过可变引用更新
                // 由于排序后顺序可能变化，我们在下面的循环中会重新处理
            }
        }

        // 字母索引
        if !skip_index {
            let first_char = entries[i]
                .name
                .chars()
                .next()
                .unwrap_or(' ')
                .to_ascii_lowercase();

            let letter_index = if first_char.is_ascii_alphabetic() {
                (first_char as u8 - b'a') as isize + 1
            } else {
                0 // 非字母字符归入第 0 组
            };

            if letter_index != alpha_index {
                alpha_index = letter_index;
                alphas.push(i);
            }

            entries[i].alpha = alphas.len().saturating_sub(1);
        }
    }

    // 再次遍历处理同名条目（需要 entries 的完整排序后才能确定）
    fix_duplicate_names(&mut entries, path, sdcard, platform_tag);

    // 计算可见窗口
    let total = entries.len();
    let visible_count = 6; // MAIN_ROW_COUNT default
    let selected = selected.min(total.saturating_sub(1));
    let start = if total <= visible_count {
        0
    } else {
        let s = selected.saturating_sub(visible_count / 2);
        s.min(total - visible_count)
    };
    let end = if total <= visible_count {
        total
    } else {
        start + visible_count
    };

    Directory {
        path: path.to_string(),
        name,
        entries,
        alphas,
        selected,
        start,
        end,
    }
}

/// 构造唯一名称 —— 对应 C 中的 `getUniqueName()`
///
/// 格式：`"DisplayName (EMU_TAG)"`
///
/// # 调用者
/// `fix_duplicate_names` → 两个同名且同文件名的条目（如相同主机不同标签的目录），
/// 用 `(EMU_TAG)` 后缀区分
fn make_unique_name(path: &str, _platform_tag: &str) -> String {
    let display = get_display_name(path);
    let tag = extract_emu_tag_from_console_path(path);
    format!("{} ({})", display, tag)
}

/// 从游戏主机目录路径中提取模拟器标签
///
/// 例如："/sdcard/Roms/Game Boy (GB)" → "GB"
///
/// # 调用者
/// `make_unique_name` → 构造 `"Name (TAG)"` 格式的唯一名称时需要标签
/// `load_recents` → 从最近游戏路径中快速提取标签以检查模拟器可用性
fn extract_emu_tag_from_console_path(path: &str) -> String {
    if let Some(paren_start) = path.rfind('(') {
        let after = &path[paren_start + 1..];
        if let Some(paren_end) = after.rfind(')') {
            return after[..paren_end].to_string();
        }
    }
    // fallback: 取最后一段
    file_name(path).unwrap_or("?").to_string()
}

/// 修复同名条目的 unique 字段（在排序后调用）
///
/// 当两个相邻条目排序后显示名相同时，根据文件名异同决定 unique 值。
///
/// # 调用者
/// `make_directory` → 条目排序后调用一次，确保所有同名条目都有区分信息
fn fix_duplicate_names(
    entries: &mut [Entry],
    _dir_path: &str,
    _sdcard: &str,
    platform_tag: &str,
) {
    let len = entries.len();
    let mut updates: Vec<(usize, Option<String>)> = Vec::new();

    for i in 0..len.saturating_sub(1) {
        if entries[i].name == entries[i + 1].name {
            let same_filename = file_name(&entries[i].path) == file_name(&entries[i + 1].path);

            let (u1, u2) = if same_filename {
                (
                    Some(make_unique_name(&entries[i].path, platform_tag)),
                    Some(make_unique_name(&entries[i + 1].path, platform_tag)),
                )
            } else {
                (
                    file_name(&entries[i].path).map(String::from),
                    file_name(&entries[i + 1].path).map(String::from),
                )
            };

            updates.push((i, u1));
            updates.push((i + 1, u2));
        }
    }

    for (idx, unique) in updates {
        if idx < entries.len() {
            entries[idx].unique = unique;
        }
    }
}

/// 应用目录级的 map.txt 名称映射
///
/// 读取目录下的 `map.txt`，应用名称映射，过滤隐藏条目，重新排序。
///
/// # 调用者
/// `make_directory` → 构建 Directory 的第一阶段，在字母索引之前调用。
/// 使得用户可以通过 map.txt 自定义 ROM 显示名，甚至隐藏某些条目。
fn apply_directory_maps(entries: &mut Vec<Entry>, dir_path: &str, _sdcard: &str) {
    // 检查当前目录的 map.txt
    let map_path = format!("{}/map.txt", dir_path);
    if path_exists(&map_path) {
        apply_name_map(entries, &map_path);
        // 过滤掉映射后应隐藏的条目
        entries.retain(|e| !hide(&e.name));
        // 重新排序
        entries.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        });
    }
}

/// 应用名称映射文件
///
/// map.txt 格式：`原始文件名<TAB>别名`
///
/// # 调用者
/// `get_root` → 对游戏主机列表应用 ROMS/map.txt 的映射（例如 "Game Boy (GB)" → "Nintendo Game Boy"）
/// `apply_directory_maps` → 对子目录内容应用 map.txt 的映射
fn apply_name_map(entries: &mut [Entry], map_path: &str) {
    let content = match std::fs::read_to_string(map_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let mut map: HashMap<String, String> = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(tab_pos) = line.find('\t') {
            let key = line[..tab_pos].to_string();
            let value = line[tab_pos + 1..].to_string();
            map.insert(key, value);
        }
    }

    let mut needs_resort = false;
    for entry in entries.iter_mut() {
        let filename = file_name(&entry.path).unwrap_or("");
        if let Some(alias) = map.get(filename) {
            entry.name = alias.clone();
            needs_resort = true;
        }
    }

    if needs_resort {
        entries.sort_by(|a, b| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        });
    }
}

// ============================================================================
// 最近游戏管理
// ============================================================================

/// 从文件加载最近游戏列表 —— 对应 C 中的 `hasRecents()`
///
/// 返回加载的 Recent 列表以及"是否有任何最近条目"的标志。
/// 同时处理 CHANGE_DISC_PATH（如果存在，将换碟路径也加入最近列表）。
///
/// 对于多碟游戏（有 m3u），只保留最后一次使用的碟。
///
/// # 调用者
/// `Menu_init`（未来） → 启动时调用一次，从 `recent.txt` 恢复用户的最近游戏历史。
/// 返回的 `has_any` 标志决定是否在主屏幕显示 "Recently Played" 条目。
pub fn load_recents(
    sdcard: &str,
    platform_tag: &str,
    paks: &str,
) -> (Vec<Recent>, bool) {
    let mut recents: Vec<Recent> = Vec::new();
    let mut parent_paths: Vec<String> = Vec::new();

    // 1. 处理换碟标记
    let change_disc_path = "/tmp/change_disc.txt";
    if path_exists(change_disc_path) {
        if let Ok(sd_path) = std::fs::read_to_string(change_disc_path) {
            let sd_path = sd_path.trim();
            if path_exists(sd_path) {
                let disc_path = sd_path
                    .strip_prefix(sdcard)
                    .unwrap_or(sd_path)
                    .to_string();
                let emu_name = extract_emu_tag_from_console_path(sd_path);
                let available = has_emu(&emu_name, sdcard, platform_tag, paks);
                if available {
                    recents.push(Recent {
                        path: disc_path.clone(),
                        alias: None,
                        available,
                    });
                }
                // 记录父目录用于多碟去重
                if let Some(parent) = parent_dir(&disc_path) {
                    parent_paths.push(parent.to_string());
                }
            }
        }
        // 读取后删除
        let _ = std::fs::remove_file(change_disc_path);
    }

    // 2. 读取 recent.txt
    let recent_file = recent_file_path(sdcard);
    let mut has_any = !recents.is_empty();

    if let Ok(content) = std::fs::read_to_string(&recent_file) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let (path, alias) = if let Some(tab_pos) = line.find('\t') {
                (&line[..tab_pos], Some(&line[tab_pos + 1..]))
            } else {
                (line, None)
            };

            let sd_path = format!("{}/{}", sdcard, path);
            if !path_exists(&sd_path) {
                continue;
            }

            if recents.len() >= MAX_RECENTS {
                break;
            }

            // 多碟游戏去重：同一个 m3u 的游戏只保留第一个
            if let Some(_m3u) = find_m3u(&sd_path) {
                let disc_parent = match parent_dir(&sd_path) {
                    Some(p) => p.to_string(),
                    None => continue,
                };
                if parent_paths.iter().any(|p| prefix_match(p, &disc_parent)) {
                    continue; // 该多碟游戏已在列表中
                }
                parent_paths.push(disc_parent);
            }

            let emu_name = get_emu_name(&sd_path, &roms_path(sdcard));
            let available = has_emu(&emu_name, sdcard, platform_tag, paks);
            if available {
                has_any = true;
            }

            recents.push(Recent {
                path: path.to_string(),
                alias: alias.map(String::from),
                available,
            });
        }
    }

    (recents, has_any)
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn setup_test_dirs(name: &str) -> String {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let base = format!("/tmp/minui_scan_test_{}_{}", name, id);
        let _ = fs::remove_dir_all(&base);

        // 创建模拟 SD 卡目录结构
        fs::create_dir_all(format!("{}/Roms/Game Boy (GB)", base)).unwrap();
        fs::create_dir_all(format!("{}/Roms/Game Boy Color (GBC)", base)).unwrap();
        fs::create_dir_all(format!("{}/Roms/Super Nintendo (SFC)", base)).unwrap();
        fs::create_dir_all(format!("{}/.system/test/paks/Emus", base)).unwrap();
        fs::create_dir_all(format!("{}/Emus/test", base)).unwrap();
        fs::create_dir_all(format!("{}/Tools/test", base)).unwrap();
        fs::create_dir_all(format!("{}/.userdata/shared/.minui", base)).unwrap();

        // 创建一些 ROM 文件
        fs::write(
            format!("{}/Roms/Game Boy (GB)/Zelda.gb", base),
            "dummy",
        ).unwrap();
        fs::write(
            format!("{}/Roms/Game Boy (GB)/Mario (World).gb", base),
            "dummy",
        ).unwrap();

        // 为 GBC 和 SFC 也创建 ROM 文件（否则 has_roms 返回 false）
        fs::write(
            format!("{}/Roms/Game Boy Color (GBC)/Pokemon.gbc", base),
            "dummy",
        ).unwrap();
        fs::write(
            format!("{}/Roms/Super Nintendo (SFC)/Zelda.sfc", base),
            "dummy",
        ).unwrap();

        fs::write(
            format!("{}/Roms/Game Boy (GB)/.hidden.gb", base),
            "dummy",
        ).unwrap();

        // 创建模拟器 Pak（简化为 launch.sh）
        fs::create_dir_all(format!("{}/.system/test/paks/Emus/GB.pak", base)).unwrap();
        fs::write(
            format!("{}/.system/test/paks/Emus/GB.pak/launch.sh", base),
            "#!/bin/sh\necho ok",
        ).unwrap();
        fs::create_dir_all(format!("{}/.system/test/paks/Emus/GBC.pak", base)).unwrap();
        fs::write(
            format!("{}/.system/test/paks/Emus/GBC.pak/launch.sh", base),
            "#!/bin/sh\necho ok",
        ).unwrap();
        fs::create_dir_all(format!("{}/.system/test/paks/Emus/SFC.pak", base)).unwrap();
        fs::write(
            format!("{}/.system/test/paks/Emus/SFC.pak/launch.sh", base),
            "#!/bin/sh\necho ok",
        ).unwrap();

        // Tools
        fs::create_dir_all(format!("{}/Tools/test/Clock.pak", base)).unwrap();
        fs::write(
            format!("{}/Tools/test/Clock.pak/launch.sh", base),
            "#!/bin/sh\necho clock",
        ).unwrap();

        base
    }

    #[test]
    fn test_scan_dir_basic() {
        let base = setup_test_dirs("unnamed");
        let gb_path = format!("{}/Roms/Game Boy (GB)", base);

        let entries = scan_dir(&gb_path, false);
        // 应该有 2 个可见文件 + 0 个隐藏文件
        assert_eq!(entries.len(), 2);

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        // Zelda → stripped extension
        assert!(names.iter().any(|n| n.contains("Zelda")));
        // Mario → stripped extension and region
        assert!(names.iter().any(|n| n.contains("Mario")));

        // 清理
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_hide_in_scan() {
        let base = setup_test_dirs("unnamed");
        let gb_path = format!("{}/Roms/Game Boy (GB)", base);

        let entries = scan_dir(&gb_path, false);
        // 不应该包含 .hidden.gb
        for entry in &entries {
            assert!(!entry.path.contains(".hidden"));
        }

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_has_emu() {
        let base = setup_test_dirs("unnamed");
        let paks = paks_path(&base, "test");

        assert!(has_emu("GB", &base, "test", &paks));
        assert!(has_emu("GBC", &base, "test", &paks));
        assert!(!has_emu("PS", &base, "test", &paks)); // 不存在的模拟器

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_get_root() {
        let base = setup_test_dirs("unnamed");
        let paks = paks_path(&base, "test");

        // 先创建一些最近游戏记录
        let recent_file = recent_file_path(&base);
        fs::create_dir_all(
            Path::new(&recent_file).parent().unwrap(),
        ).unwrap();
        fs::write(&recent_file, "").unwrap();

        let root = get_root(&base, "test", &paks, false, false, false);

        // 应该有 GB, GBC, SFC 三个系统 + Tools
        assert!(root.len() >= 4);

        let names: Vec<&str> = root.iter().map(|e| e.name.as_str()).collect();
        assert!(names.iter().any(|n| n.contains("Game Boy")), "Expected Game Boy in {:?}", names);
        assert!(names.iter().any(|n| n.contains("Super Nintendo")), "Expected Super Nintendo in {:?}", names);

        // 验证排序（大小写不敏感）
        let lower_names: Vec<String> = root.iter()
            .map(|e| e.name.to_ascii_lowercase())
            .collect();
        let mut sorted = lower_names.clone();
        sorted.sort();
        assert_eq!(lower_names, sorted, "Root entries should be sorted");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_get_root_simple_mode() {
        let base = setup_test_dirs("unnamed");
        let paks = paks_path(&base, "test");

        // simple_mode = true → 不应该包含 Tools
        let root = get_root(&base, "test", &paks, false, false, true);

        let names: Vec<&str> = root.iter().map(|e| e.name.as_str()).collect();
        assert!(!names.iter().any(|n| n.contains("Tools")), "Simple mode should hide Tools");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_find_m3u() {
        let base = setup_test_dirs("unnamed");
        // 创建多碟游戏结构
        fs::create_dir_all(format!("{}/Roms/PS/Final Fantasy VII", base)).unwrap();
        fs::write(
            format!("{}/Roms/PS/Final Fantasy VII/Disc 1.cue", base),
            "dummy",
        ).unwrap();
        fs::write(
            format!("{}/Roms/PS/Final Fantasy VII.m3u", base),
            "Final Fantasy VII/Disc 1.cue\n",
        ).unwrap();

        let disc_path = format!("{}/Roms/PS/Final Fantasy VII/Disc 1.cue", base);
        let m3u = find_m3u(&disc_path);
        assert!(m3u.is_some());
        assert!(m3u.unwrap().ends_with("Final Fantasy VII.m3u"));

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_get_discs() {
        let base = setup_test_dirs("unnamed");
        fs::create_dir_all(format!("{}/Roms/PS/Final Fantasy VII", base)).unwrap();
        fs::write(
            format!("{}/Roms/PS/Final Fantasy VII/Disc 1.cue", base),
            "dummy",
        ).unwrap();
        fs::write(
            format!("{}/Roms/PS/Final Fantasy VII/Disc 2.cue", base),
            "dummy",
        ).unwrap();
        fs::write(
            format!("{}/Roms/PS/Final Fantasy VII.m3u", base),
            "Final Fantasy VII/Disc 1.cue\nFinal Fantasy VII/Disc 2.cue\n",
        ).unwrap();

        let m3u_path = format!("{}/Roms/PS/Final Fantasy VII.m3u", base);
        let discs = get_discs(&m3u_path);
        assert_eq!(discs.len(), 2);
        assert_eq!(discs[0].name, "Disc 1");
        assert_eq!(discs[1].name, "Disc 2");

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_make_directory_indexing() {
        let base = setup_test_dirs("unnamed");
        let gb_path = format!("{}/Roms/Game Boy (GB)", base);

        let entries = scan_dir(&gb_path, false);
        let dir = make_directory(&gb_path, entries, 0, &base, "test");

        // 两个 ROM 的首字母不同 → 应该有对应的字母索引
        assert!(!dir.entries.is_empty());
        assert!(!dir.alphas.is_empty());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_map_txt_application() {
        let base = setup_test_dirs("unnamed");

        // 创建 map.txt
        fs::write(
            format!("{}/Roms/map.txt", base),
            "Game Boy (GB)\tNintendo Game Boy\n",
        ).unwrap();

        let paks = paks_path(&base, "test");
        let root = get_root(&base, "test", &paks, false, false, false);

        // 应该有被重命名的条目
        let names: Vec<&str> = root.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.iter().any(|n| n.contains("Nintendo")),
            "Expected renamed entry in {:?}",
            names
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_extract_collate_prefix() {
        // "Game Boy (GB)" 的归类前缀以 "( " 结尾
        let prefix = extract_collate_prefix("/sdcard/Roms/Game Boy (GB)");
        assert!(prefix.ends_with("("), "prefix should end with '(': {}", prefix);
        // 应该匹配 "Game Boy (GBC)"（同一系列，不同标签），因为两者都以 "Game Boy (" 开头
        assert!(prefix_match(&prefix, "/sdcard/Roms/Game Boy (GBC)"));
        // 不应该匹配 "Game Boy Color (GBC)"（多了 "Color" 这个词，"(" ≠ "C"）
        assert!(!prefix_match(&prefix, "/sdcard/Roms/Game Boy Color (GBC)"));
    }
}
