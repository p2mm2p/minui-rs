//! 最近游戏管理
//!
//! 维护最近玩过的 ROM 列表（最多 24 条），持久化到
//! `/.userdata/shared/.minui/recent.txt`。
//! 对应原版 C `Recent` 结构体和 `addRecent()`/`saveRecents()`/
//! `hasRecents()` 逻辑（minui.c:377-418/:441-599）。
//!
//! ## 依赖方向
//!
//! 本模块只依赖 `common`（utils/paths/协议常量）与 `disc`
//! （`find_m3u`/`has_emu`），不依赖 launch/browser。
//!
//! ## available_when_load 语义
//!
//! `Recent::available_when_load` 是**加载时刻**的瞬时快照（模拟器是否
//! 仍安装），不持久化——每次 `find_recents` 重新计算。加载**保留**
//! 不可用条目，显示过滤是 browser 的 Recent→Entry 转换职责（下次
//! 变更），模拟器重装后条目自动恢复显示。
//!

#![cfg_attr(not(feature = "platform-tg5040"), allow(dead_code))] // 无平台 feature（测试编译）时保留 allow

use common::paths::{CHANGE_DISC_PATH, get_recent_path, get_roms_path};
use common::utils::{exists, get_emu_name, get_file, prefix_match, put_file, remove_file};

use crate::disc;

/// 最近游戏条目的数量上限
///
/// 对应原 C `MAX_RECENTS`（`minui.c:440`——"a multiple of all menu rows"，
/// 必须是所有菜单行数的倍数，24 是 6/8 行的公倍数）。
pub(crate) const MAX_RECENTS: usize = 24;

/// 最近游戏条目
///
/// 对应原 C `Recent` 结构体（`minui.c:377-381`）。
#[derive(Debug, PartialEq)]
pub(crate) struct Recent {
    /// 无 `SDCARD_PATH` 前缀的相对路径（如 `"/Roms/SFC/Game.sfc"`——**保留前导 `/`**）
    ///
    /// 持久化格式（recent.txt 每行第一列）与 `add_recent` 的比较键。
    /// C 的 `addRecent` 用指针偏移去掉前缀（`path += strlen(SDCARD_PATH)`），
    /// 完整路径 `/SDCARD/Roms/...` 去前缀后为 `/Roms/...`（带前导 `/`）——
    /// 本字段忠实还原该格式。使用时由调用方拼接前缀。**平台无关**
    /// （C 注释特别强调，minui.c:378）。
    pub path: String,
    /// 可选显示别名（recent.txt 的 `\t` 第二列）
    ///
    /// 显示时覆盖条目的显示名称（对应 C `getRecents` 中
    /// `entry->name = strdup(recent->alias)`，minui.c:764-767）。
    pub alias: Option<String>,
    /// 加载时刻模拟器是否仍安装（瞬时快照，不持久化；显示时过滤）
    ///
    /// 每次加载/创建时用 `disc::has_emu(get_emu_name(path))` 实时计算
    /// （对应 C `Recent_new`，minui.c:397）。`find_recents` **保留**
    /// 不可用条目；过滤发生在 browser 的 Recent→Entry 转换时。
    pub available_when_load: bool,
}

impl Recent {
    /// 创建最近条目（计算 `available_when_load`）
    ///
    /// 对应原 C `Recent_new()`（minui.c:386-399）：拼 SDCARD 前缀 →
    /// `getEmuName` → `hasEmu`。
    fn new(
        path: &str,
        alias: Option<String>,
        sdcard_path: &str,
        platform: &str,
        paks_path: &str,
    ) -> Self {
        let sd_path = format!("{sdcard_path}{path}");
        let roms_path = get_roms_path(sdcard_path);
        let emu_name = get_emu_name(&sd_path, &roms_path);
        let available = disc::has_emu(&emu_name, sdcard_path, platform, paks_path);
        Self {
            path: path.to_string(),
            alias,
            available_when_load: available,
        }
    }
}

/// 加载最近游戏列表
///
/// 对应原 C `hasRecents()`（minui.c:518-599）的"加载 + 可用性判定"
/// 合一，改为纯函数返回 `Option<Vec<Recent>>`（消灭全局 recents 数组
/// 副作用）。流程：
/// 1. `CHANGE_DISC_PATH` 换碟请求注入列表顶部并删除文件（C :523-538）
/// 2. 读 recent.txt 逐行（`path[\talias]`），过滤不存在的条目（C :541-593）
/// 3. 多碟去重：同父目录的多个碟片只保留一条（C :563-582）
/// 4. 回写规范化文件（清理不存在的条目，C :595 的 saveRecents）
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径（`Platform::SDCARD_PATH`）
/// - `platform`:平台代码（`Platform::PLATFORM`）
/// - `paks_path`:平台 paks 目录（`common::paths::get_paks_path`）
///
/// # 返回值
///
/// `None`——没有任何**可用**（`available_when_load=true`）的最近游戏
/// （调用方不建 Recently Played 伪目录）；`Some(vec)`——存在可用条目，
/// vec 包含**全部**条目（含不可用，由显示层过滤）。
pub(crate) fn find_recents(
    sdcard_path: &str,
    platform: &str,
    paks_path: &str,
) -> Option<Vec<Recent>> {
    let mut recents: Vec<Recent> = Vec::new();
    let mut parent_paths: Vec<String> = Vec::new();

    // ── 1. CHANGE_DISC_PATH 换碟请求注入（C minui.c:523-538）──
    if exists(CHANGE_DISC_PATH) {
        if let Some(content) = get_file(CHANGE_DISC_PATH) {
            let content = content.trim().to_string();
            if !content.is_empty() && exists(&content) {
                // 去 SDCARD 前缀（C :527 指针偏移）
                let disc_path = content
                    .strip_prefix(sdcard_path)
                    .unwrap_or(&content)
                    .to_string();
                let recent = Recent::new(&disc_path, None, sdcard_path, platform, paks_path);
                recents.push(recent);
                // 登记父目录（C :532-536）
                if let Some((p, _)) = disc_path.rsplit_once('/') {
                    parent_paths.push(format!("{p}/"));
                }
            }
        }
        let _ = remove_file(CHANGE_DISC_PATH); // C :538 unlink
    }

    // ── 2-4. recent.txt 加载 + 多碟去重 + 回写（C :541-595）──
    let recent_path = get_recent_path(sdcard_path);
    if let Some(file) = get_file(&recent_path) {
        for line in file.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (path, alias) = match line.split_once('\t') {
                Some((p, a)) => (p, Some(a.trim().to_string())),
                None => (line, None),
            };

            let sd_path = format!("{sdcard_path}{path}");
            if !exists(&sd_path) {
                continue; // C :561
            }
            if recents.len() >= MAX_RECENTS {
                break; // C :562 的 count<MAX_RECENTS 条件
            }

            // 多碟去重（C :563-582）——父目录已登记则跳过
            if disc::find_m3u(&sd_path).is_some() {
                let parent = match path.rsplit_once('/') {
                    Some((p, _)) => format!("{p}/"),
                    None => continue,
                };
                if parent_paths.iter().any(|pp| prefix_match(pp, &parent)) {
                    continue;
                }
                parent_paths.push(parent);
            }

            let recent = Recent::new(path, alias, sdcard_path, platform, paks_path);
            recents.push(recent);
        }
    }

    // 回写规范化文件（C :595 saveRecents——清理不存在的条目）
    save_recents(&recents, sdcard_path);

    if recents.iter().any(|r| r.available_when_load) {
        Some(recents)
    } else {
        None
    }
}

/// 记录一次游玩
///
/// 对应原 C `addRecent()`（minui.c:456-473）：`path` 去除 SDCARD 前缀
/// 后作为比较键——不存在则插入顶部并截断尾部（`MAX_RECENTS=24`）；
/// 已存在则 bump 到顶部；随后持久化。
///
/// # 参数
///
/// - `recents`:最近列表（调用方持有的可变引用）
/// - `path`:本次游玩的完整路径（**带** SDCARD 前缀——内部去除存储）
/// - `alias`:可选显示别名（对应 C 的 `recent_alias` 全局，显式参数化）
/// - `sdcard_path`/`platform`/`paks_path`:平台原语（供 `Recent::new`）
pub(crate) fn add_recent(
    recents: &mut Vec<Recent>,
    path: &str,
    alias: Option<&str>,
    sdcard_path: &str,
    platform: &str,
    paks_path: &str,
) {
    // 去 SDCARD 前缀（C :457 `path += strlen(SDCARD_PATH)`——strip_prefix 更安全，
    // 不匹配时保持原样，C 在路径不带前缀时会越界读取）
    let stripped = path.strip_prefix(sdcard_path).unwrap_or(path);

    match recents.iter().position(|r| r.path == stripped) {
        None => {
            // 截断尾部（C :460-462）
            if recents.len() >= MAX_RECENTS {
                recents.pop();
            }
            // 插入顶部（C :463 Array_unshift）
            let recent = Recent::new(
                stripped,
                alias.map(|a| a.to_string()),
                sdcard_path,
                platform,
                paks_path,
            );
            recents.insert(0, recent);
        }
        Some(0) => {} // 已在顶部，无需 bump（C :465 的 id>0 条件）
        Some(id) => {
            // bump 到顶部（C :465-470）
            let item = recents.remove(id);
            recents.insert(0, item);
        }
    }

    save_recents(recents, sdcard_path);
}

/// 持久化最近列表到 recent.txt
///
/// 对应原 C `saveRecents()`（minui.c:441-455）：每行 `path` 或
/// `path\talias`，以 `\n` 结尾。写入失败静默忽略（C 的 `fopen` 失败
/// 同样静默）。
fn save_recents(recents: &[Recent], sdcard_path: &str) {
    let recent_path = get_recent_path(sdcard_path);
    let mut content = String::new();
    for r in recents {
        content.push_str(&r.path);
        if let Some(alias) = &r.alias {
            content.push('\t');
            content.push_str(alias);
        }
        content.push('\n');
    }
    let _ = put_file(&recent_path, &content);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 生成测试 SD 卡根目录（正斜杠路径——Windows 的 `std::fs` 接受 `/`，
    /// 且与真实设备上的路径格式一致）
    fn temp_root(name: &str) -> String {
        let base = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        format!("{base}/minui_recents_{}_{name}", std::process::id())
    }

    /// 构造 SD 卡目录结构（Roms + .userdata/shared/.minui）
    fn setup_sdcard(root: &str) {
        fs::create_dir_all(format!("{root}/.userdata/shared/.minui")).unwrap();
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
    }

    /// 安装模拟器（优先候选路径）
    fn install_emu(root: &str, platform: &str, emu: &str) {
        let pak = format!("{root}/Emus/{platform}/{emu}.pak/launch.sh");
        fs::create_dir_all(std::path::Path::new(&pak).parent().unwrap()).unwrap();
        fs::write(&pak, "#!/bin/sh\n").unwrap();
    }

    const PLATFORM: &str = "tg5040";

    /// `CHANGE_DISC_PATH`（/tmp/change_disc.txt）是**全局共享**文件——
    /// 所有调用 `find_recents` 的测试都会读它，并行时会误读其他测试
    /// 写入的换碟请求，必须串行。纯 std 方案（静态 Mutex）。

    // ── find_recents ────────────────────────────

    #[test]
    fn find_recents_no_file_returns_none() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("no_file");
        setup_sdcard(&root);

        let result = find_recents(&root, PLATFORM, &format!("{root}/.system/{PLATFORM}/paks"));
        assert_eq!(result, None);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn find_recents_keeps_unavailable_entries() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("keep_unavailable");
        setup_sdcard(&root);
        // 两个条目：GB 模拟器已安装；SFC 未安装
        install_emu(&root, PLATFORM, "GB");
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        let _ = fs::create_dir_all(format!("{root}/Roms/Game Boy (GB)"));
        fs::write(format!("{root}/Roms/Game Boy (GB)/Pokemon Red.gb"), "x").unwrap();
        fs::write(
            format!("{root}/.userdata/shared/.minui/recent.txt"),
            "/Roms/Game Boy (GB)/Pokemon Red.gb\n/Roms/SFC/Game.sfc\n",
        )
        .unwrap();

        let result = find_recents(&root, PLATFORM, &format!("{root}/.system/{PLATFORM}/paks"));
        let recents = result.expect("有可用条目应返回 Some");
        assert_eq!(recents.len(), 2, "不可用条目应保留");
        assert!(recents[0].available_when_load);
        assert!(!recents[1].available_when_load);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn find_recents_all_unavailable_returns_none() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("all_unavailable");
        setup_sdcard(&root);
        let _ = fs::create_dir_all(format!("{root}/Roms/SFC"));
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        fs::write(
            format!("{root}/.userdata/shared/.minui/recent.txt"),
            "/Roms/SFC/Game.sfc\n",
        )
        .unwrap();

        let result = find_recents(&root, PLATFORM, &format!("{root}/.system/{PLATFORM}/paks"));
        assert_eq!(result, None);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn find_recents_change_disc_injection() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("change_disc");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "GB");
        let _ = fs::create_dir_all(format!("{root}/Roms/Game Boy (GB)"));
        fs::write(format!("{root}/Roms/Game Boy (GB)/Pokemon Red.gb"), "x").unwrap();
        fs::write(
            format!("{root}/.userdata/shared/.minui/recent.txt"),
            "/Roms/SFC/Other.sfc\n",
        )
        .unwrap();

        // 换碟请求：带前缀完整路径（minarch.c:380 的写入格式）
        let _ = fs::create_dir_all("/tmp");
        fs::write(
            CHANGE_DISC_PATH,
            format!("{root}/Roms/Game Boy (GB)/Pokemon Red.gb"),
        )
        .unwrap();

        let result = find_recents(&root, PLATFORM, &format!("{root}/.system/{PLATFORM}/paks"));
        let recents = result.expect("换碟条目可用应返回 Some");
        // 换碟条目在顶部（存储格式保留前导 /，见 add_recent 注释）
        assert_eq!(recents[0].path, "/Roms/Game Boy (GB)/Pokemon Red.gb");
        assert!(!exists(CHANGE_DISC_PATH), "换碟请求文件应被删除");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn find_recents_multi_disc_dedup() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("dedup");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "SFC");
        let game_dir = format!("{root}/Roms/SFC/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(
            format!("{game_dir}/Final Fantasy VII.m3u"),
            "Disc 1.sfc\nDisc 2.sfc\n",
        )
        .unwrap();
        fs::write(format!("{game_dir}/Disc 1.sfc"), "x").unwrap();
        fs::write(format!("{game_dir}/Disc 2.sfc"), "x").unwrap();
        fs::write(
            format!("{root}/.userdata/shared/.minui/recent.txt"),
            "/Roms/SFC/Final Fantasy VII/Disc 1.sfc\n/Roms/SFC/Final Fantasy VII/Disc 2.sfc\n",
        )
        .unwrap();

        let result = find_recents(&root, PLATFORM, &format!("{root}/.system/{PLATFORM}/paks"));
        let recents = result.expect("应有可用条目");
        assert_eq!(recents.len(), 1, "多碟去重：只保留一条");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn find_recents_rewrites_missing_entries() {
        let _guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let root = temp_root("rewrite");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "GB");
        let _ = fs::create_dir_all(format!("{root}/Roms/Game Boy (GB)"));
        fs::write(format!("{root}/Roms/Game Boy (GB)/Pokemon Red.gb"), "x").unwrap();
        fs::write(
            format!("{root}/.userdata/shared/.minui/recent.txt"),
            "/Roms/Game Boy (GB)/Pokemon Red.gb\n/Roms/SFC/Gone.sfc\n",
        )
        .unwrap();

        let result = find_recents(&root, PLATFORM, &format!("{root}/.system/{PLATFORM}/paks"));
        assert!(result.is_some());
        // 回写规范化：Gone.sfc（不存在）被清理
        let saved =
            fs::read_to_string(format!("{root}/.userdata/shared/.minui/recent.txt")).unwrap();
        assert!(!saved.contains("Gone.sfc"));
        assert!(saved.contains("Pokemon Red.gb"));

        fs::remove_dir_all(&root).unwrap();
    }

    // ── add_recent ──────────────────────────────

    #[test]
    fn add_recent_insert_top() {
        let root = temp_root("add_insert");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "GB");
        let mut recents: Vec<Recent> = Vec::new();

        add_recent(
            &mut recents,
            &format!("{root}/Roms/Game Boy (GB)/Pokemon Red.gb"),
            Some("Pokemon Red"),
            &root,
            PLATFORM,
            &format!("{root}/.system/{PLATFORM}/paks"),
        );

        assert_eq!(recents.len(), 1);
        // 存储格式 = 去掉 SDCARD 前缀后**保留前导 /**（C `path += strlen(SDCARD_PATH)`
        // 的指针偏移语义——完整路径 "/SDCARD/Roms/..." 去前缀后为 "/Roms/..."）
        assert_eq!(recents[0].path, "/Roms/Game Boy (GB)/Pokemon Red.gb");
        assert_eq!(recents[0].alias.as_deref(), Some("Pokemon Red"));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn add_recent_bump_existing() {
        let root = temp_root("add_bump");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "GB");
        let mut recents: Vec<Recent> = vec![
            Recent {
                path: "/Roms/SFC/A.sfc".to_string(),
                alias: None,
                available_when_load: true,
            },
            Recent {
                path: "/Roms/SFC/B.sfc".to_string(),
                alias: None,
                available_when_load: true,
            },
        ];

        add_recent(
            &mut recents,
            &format!("{root}/Roms/SFC/B.sfc"),
            None,
            &root,
            PLATFORM,
            &format!("{root}/.system/{PLATFORM}/paks"),
        );

        assert_eq!(recents.len(), 2, "不重复添加");
        assert_eq!(recents[0].path, "/Roms/SFC/B.sfc", "bump 到顶部");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn add_recent_truncates_at_max() {
        let root = temp_root("add_truncate");
        setup_sdcard(&root);
        let mut recents: Vec<Recent> = (0..MAX_RECENTS)
            .map(|i| Recent {
                path: format!("/Roms/SFC/Game{i}.sfc"),
                alias: None,
                available_when_load: true,
            })
            .collect();

        add_recent(
            &mut recents,
            &format!("{root}/Roms/SFC/New.sfc"),
            None,
            &root,
            PLATFORM,
            &format!("{root}/.system/{PLATFORM}/paks"),
        );

        assert_eq!(recents.len(), MAX_RECENTS, "上限保持 24");
        assert_eq!(recents[0].path, "/Roms/SFC/New.sfc");
        assert_eq!(
            recents.last().unwrap().path,
            "/Roms/SFC/Game22.sfc",
            "最旧的被弹出"
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn save_format_path_tab_alias() {
        let root = temp_root("save_format");
        setup_sdcard(&root);
        let recents = vec![
            Recent {
                path: "/Roms/A.gb".to_string(),
                alias: Some("Alias".to_string()),
                available_when_load: true,
            },
            Recent {
                path: "/Roms/B.sfc".to_string(),
                alias: None,
                available_when_load: true,
            },
        ];

        save_recents(&recents, &root);
        let saved =
            fs::read_to_string(format!("{root}/.userdata/shared/.minui/recent.txt")).unwrap();
        assert_eq!(saved, "/Roms/A.gb\tAlias\n/Roms/B.sfc\n");

        fs::remove_dir_all(&root).unwrap();
    }
}
