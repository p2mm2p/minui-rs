//! 文件浏览器 — `Directory`/`Entry` 管理与目录索引
//!
//! 对应原版 C `minui.c` 的目录/条目区（:105-373 数据结构、:214-322 索引、
//! :600-942 构造器）。本模块负责"路径 → 条目列表"的数据装配：
//!
//! - `Directory::new` 按路径形态分发到对应条目构造器（根目录 / 最近游玩 /
//!   合集 .txt / m3u 碟片 / 普通目录）
//! - 条目构造全部使用 `std::fs::read_dir` **单层**扫描（对应 C `opendir`/
//!   `readdir` 语义——子目录作为 `EntryType::Dir` 条目由用户逐层导航）
//! - `directory_index` 在构造后执行 map.txt 别名重映射、hide 过滤、重名
//!   unique 后缀与字母索引（`alphas`）构建
//!
//! ## 依赖方向
//!
//! 本模块 SHALL 只依赖 `common`（paths/utils）、`crate::disc`（`has_emu`）
//! 与 `crate::recents`（`find_recents`/`Recent`）——不依赖 launch/render/
//! 平台 crate。方向单向无环：`common ← disc ← recents ← browser`。
//!

#![cfg_attr(not(feature = "tg5040"), allow(dead_code))] // 无平台 feature（测试编译）时保留 allow

use std::collections::HashMap;

use common::paths::{
    get_collections_path, get_faux_recent_path, get_roms_path, get_simple_mode_path, get_tools_dir,
};
use common::utils::{
    exact_match, exists, get_display_name, get_emu_name, get_file, hide, prefix_match, suffix_match,
};

use crate::disc;
use crate::recents::{self, Recent};

/// 文件条目类型
///
/// 对应原 C `EntryType` 枚举（minui.c:105-109）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryType {
    /// 目录（可进入浏览）
    Dir,
    /// .pak 模拟器/工具包目录
    Pak,
    /// ROM 文件
    Rom,
}

/// 单个文件条目
///
/// 对应原 C `Entry` 结构体（minui.c:110-116）。
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    /// SD 卡上的完整路径
    pub path: String,
    /// 显示名称（经 `get_display_name` 去除扩展名/括号标记/排序前缀）
    pub name: String,
    /// 多碟/重名条目的唯一标识后缀（无重名时为 `None`）
    pub unique: Option<String>,
    /// 条目所属字母组在父 `Directory::alphas` 中的下标（C :115 注释语义，
    /// L1/R1 跳字母时经它取 `alphas[alpha±1]`）
    pub alpha: usize,
    /// 条目类型
    pub entry_type: EntryType,
}

/// 目录（当前浏览的文件夹）
///
/// 对应原 C `Directory` 结构体（minui.c:181-190）。`start`/`end` 为滚动
/// 窗口的页首/页尾条目下标——本模块只声明字段并给出初始值，滚动计算
/// 属 main 变更。
#[derive(Debug, Clone)]
pub(crate) struct Directory {
    /// 目录路径
    pub path: String,
    /// 目录显示名
    ///
    /// 对应 C `Directory::name`（minui.c:332，`getDisplayName` 写入）——
    /// C 原版同样从未读取该字段（列表页无标题栏），Rust 版忠实保留
    /// 结构对齐。
    #[allow(dead_code)]
    pub name: String,
    /// 包含的条目列表（已排序、已索引）
    pub entries: Vec<Entry>,
    /// 当前选中的条目索引
    pub selected: usize,
    /// 滚动窗口页首条目下标
    pub start: usize,
    /// 滚动窗口页尾条目下标（开区间）
    pub end: usize,
    /// 字母组起始条目下标的有序集合（`alphas[i]` = 第 i 个字母组的首个
    /// 条目在 `entries` 中的下标；对应 C `IntArray`，容量上限 27 的
    /// Rust 化——`Vec<usize>` 免去固定容量魔法数）
    pub alphas: Vec<usize>,
}

impl Directory {
    /// 按路径形态分发构造目录
    ///
    /// 对应原 C `Directory_new`（minui.c:330-356）。分发规则（按序判断）：
    /// 1. `path == sdcard_path` → 根目录（`get_root`：Recently Played 伪目录、
    ///    Roms console 目录、Collections、Tools）
    /// 2. `path == get_faux_recent_path` → 最近游玩条目（`get_recents`）
    /// 3. `Collections` 前缀 + `.txt` 后缀 → 合集条目（`get_collection`）
    /// 4. `.m3u` 后缀 → 碟片条目（`get_discs`）
    /// 5. 其他 → 目录条目（`get_entries`，含 console 目录合并）
    ///
    /// 构造后执行索引（map.txt/unique/alphas）。平台原语由调用方从
    /// `Platform` trait 关联常量显式传入——本模块不依赖平台 crate。
    ///
    /// # 参数
    ///
    /// - `path`:目录路径（绝对路径）
    /// - `selected`:初始选中条目下标（透传，main 的 restore 会传历史值）
    /// - `sdcard_path`:SD 卡根路径（`Platform::SDCARD_PATH`）
    /// - `platform`:平台代码（`Platform::PLATFORM`）
    /// - `paks_path`:平台 paks 目录（`common::paths::get_paks_path`）
    ///
    /// # 返回值
    ///
    /// 条目已排序、已索引的 `Directory`。
    pub fn new(
        path: &str,
        selected: usize,
        sdcard_path: &str,
        platform: &str,
        paks_path: &str,
    ) -> Self {
        let name = get_display_name(path, platform);
        let entries = if exact_match(path, sdcard_path) {
            get_root(sdcard_path, platform, paks_path)
        } else if exact_match(path, &get_faux_recent_path(sdcard_path)) {
            match recents::find_recents(sdcard_path, platform, paks_path) {
                Some(r) => get_recents(&r, sdcard_path, platform),
                None => Vec::new(),
            }
        } else if prefix_match(&get_collections_path(sdcard_path), path)
            && suffix_match(".txt", path)
        {
            get_collection(path, sdcard_path, platform)
        } else if suffix_match(".m3u", path) {
            get_discs(path, platform)
        } else {
            get_entries(path, sdcard_path, platform)
        };
        let end = entries.len();
        let mut dir = Directory {
            path: path.to_string(),
            name,
            entries,
            selected,
            start: 0,
            end,
            alphas: Vec::new(),
        };
        directory_index(&mut dir, sdcard_path);
        dir
    }
}

/// 创建条目（显示名经 `get_display_name` 处理）
///
/// 对应原 C `Entry_new`（minui.c:118-128）：路径、显示名、类型 + `alpha`
/// 初始 0（索引阶段覆盖）。`platform` 供 `get_display_name` 剥离末尾平台段
/// （如 Tools 路径）。
fn entry_new(path: &str, entry_type: EntryType, platform: &str) -> Entry {
    Entry {
        path: path.to_string(),
        name: get_display_name(path, platform),
        unique: None,
        alpha: 0,
        entry_type,
    }
}

/// 按显示名不区分大小写排序
///
/// 对应 C `EntryArray_sort`（minui.c:143-150，`strcasecmp`）。`to_ascii_lowercase`
/// 与 `strcasecmp` 同为 ASCII 大小写折叠——不做 Unicode 全折叠（避免引入
/// C 没有的行为，如 İ→i̇ 多字符展开）。
fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
}

/// console 目录判断：父目录 == Roms 目录
///
/// 对应 C `isConsoleDir`（minui.c:899-907）。console 目录是 Roms 下的
/// 第一层子目录（如 `Roms/Game Boy (GB)`），`get_entries` 对它们执行
/// collate 合并。
fn is_console_dir(path: &str, sdcard_path: &str) -> bool {
    let roms_path = get_roms_path(sdcard_path);
    match path.rsplit_once('/') {
        Some((parent, _)) => exact_match(&roms_path, parent),
        None => false,
    }
}

/// console 目录（父目录 == Roms）是否可用：模拟器已装 + 目录非空
///
/// 对应 C `hasRoms`（minui.c:614-638）。两个条件缺一不可：
/// 1. `get_emu_name` 推断的模拟器 `.pak` 已安装（`disc::has_emu`）
/// 2. 目录内存在至少一个非隐藏条目（假定为 ROM）
///
/// # 参数
///
/// - `dir_name`:console 目录名（如 `"Game Boy (GB)"`，不带路径）
/// - `sdcard_path`:SD 卡根路径
/// - `platform`:平台代码
/// - `paks_path`:平台 paks 目录
///
/// # 返回值
///
/// 两个条件都满足 → `true`；任一不满足 → `false`。
fn has_roms(dir_name: &str, sdcard_path: &str, platform: &str, paks_path: &str) -> bool {
    let roms_path = get_roms_path(sdcard_path);
    let dir_path = format!("{roms_path}/{dir_name}");
    let emu_name = get_emu_name(&dir_path, &roms_path);
    if !disc::has_emu(&emu_name, sdcard_path, platform, paks_path) {
        return false;
    }
    match std::fs::read_dir(&dir_path) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .any(|e| !hide(&e.file_name().to_string_lossy())),
        Err(_) => false,
    }
}

/// Collections 目录是否存在可见条目
///
/// 对应 C `hasCollections`（minui.c:600-613）：目录存在且至少一个非隐藏条目。
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径
///
/// # 返回值
///
/// 存在可见条目 → `true`；目录不存在或仅隐藏条目 → `false`。
fn has_collections(sdcard_path: &str) -> bool {
    let collections_path = get_collections_path(sdcard_path);
    match std::fs::read_dir(&collections_path) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .any(|e| !hide(&e.file_name().to_string_lossy())),
        Err(_) => false,
    }
}

/// 单层扫描目录并把条目追加到列表
///
/// 对应 C `addEntries`（minui.c:863-897）。`std::fs::read_dir` 单层遍历
/// （不递归——子目录作为条目由用户逐层导航）。条目类型判定：
///
/// - 目录 → 后缀 `.pak` 判 `Pak`，否则 `Dir`
/// - 文件 → 位于 Collections 下判 `Dir`（C :886-888 的 ":shrug:"——合集
///   顶层文件视作目录语义），否则 `Rom`
///
/// # 参数
///
/// - `entries`:追加目标列表（可变借用）
/// - `path`:要扫描的目录路径
/// - `sdcard_path`:SD 卡根路径（Collections 前缀判定用）
/// - `platform`:平台代码（`entry_new` 显示名处理用）
fn add_entries(entries: &mut Vec<Entry>, path: &str, sdcard_path: &str, platform: &str) {
    let Ok(rd) = std::fs::read_dir(path) else {
        return;
    };
    let collections_path = get_collections_path(sdcard_path);
    for item in rd.flatten() {
        let name = item.file_name().to_string_lossy().to_string();
        if hide(&name) {
            continue;
        }
        let full_path = format!("{path}/{name}");
        let is_dir = item.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let entry_type = if is_dir {
            if suffix_match(".pak", &name) {
                EntryType::Pak
            } else {
                EntryType::Dir
            }
        } else if prefix_match(&collections_path, &full_path) {
            EntryType::Dir // C :886-888
        } else {
            EntryType::Rom
        };
        entries.push(entry_new(&full_path, entry_type, platform));
    }
}

/// 目录条目构造（含 console 目录 collate 合并）
///
/// 对应 C `getEntries`（minui.c:909-942）。console 目录（`is_console_dir`）
/// 执行合并：把 path 截断到最后一个 `(` 处（**保留** `(`——C 注释 :916-918：
/// 避免 "Game Boy" 合并掉 "Game Boy Color"/"Game Boy Advance"）作为
/// collated 前缀，遍历 Roms 下全部子目录，前缀命中者逐目录 `add_entries`；
/// 非 console 目录只扫描自身。结果统一排序。
///
/// # 参数
///
/// - `path`:要打开的目录路径
/// - `sdcard_path`:SD 卡根路径
/// - `platform`:平台代码
///
/// # 返回值
///
/// 排序后的条目列表。
fn get_entries(path: &str, sdcard_path: &str, platform: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    if is_console_dir(path, sdcard_path) {
        let mut collated = path.to_string();
        if let Some(paren) = collated.rfind('(') {
            collated.truncate(paren + 1); // 保留 '('（C :917）
        }
        let roms_path = get_roms_path(sdcard_path);
        if let Ok(rd) = std::fs::read_dir(&roms_path) {
            for sub in rd.flatten() {
                let name = sub.file_name().to_string_lossy().to_string();
                if hide(&name) {
                    continue;
                }
                let sub_path = format!("{roms_path}/{name}");
                let is_dir = sub.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if !is_dir || !prefix_match(&collated, &sub_path) {
                    continue;
                }
                add_entries(&mut entries, &sub_path, sdcard_path, platform);
            }
        }
    } else {
        add_entries(&mut entries, path, sdcard_path, platform);
    }
    sort_entries(&mut entries);
    entries
}

/// 最近游玩 Recent → Entry 转换
///
/// 对应 C `getRecents`（minui.c:754-771）。**过滤** `available_when_load ==
/// false` 的条目（recents 模块 spec 声明的跨模块边界：显示过滤在 Recent→
/// Entry 转换时）。类型判定：`.pak` 后缀 → `Pak`，否则 `Rom`（C :762）。
/// `Recent.alias` 存在时覆盖 `Entry.name`（C :764-767）。
///
/// # 参数
///
/// - `recents`:最近游玩列表（`recents::find_recents` 的返回值）
/// - `sdcard_path`:SD 卡根路径（拼接完整路径）
/// - `platform`:平台代码
///
/// # 返回值
///
/// 过滤并转换后的条目列表。
fn get_recents(recents: &[Recent], sdcard_path: &str, platform: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    for recent in recents {
        if !recent.available_when_load {
            continue;
        }
        let sd_path = format!("{sdcard_path}{}", recent.path);
        let entry_type = if suffix_match(".pak", &sd_path) {
            EntryType::Pak
        } else {
            EntryType::Rom
        };
        let mut entry = entry_new(&sd_path, entry_type, platform);
        if let Some(alias) = &recent.alias {
            entry.name = alias.clone();
        }
        entries.push(entry);
    }
    entries
}

/// 合集 .txt → 条目转换
///
/// 对应 C `getCollection`（minui.c:772-798）。逐行解析（跳过空行），行内容
/// 为无 SDCARD 前缀的相对路径（如 `/Roms/SFC/A.sfc`）→ 拼前缀 → `exists`
/// 过滤 → `.pak` 后缀判 `Pak` 否则 `Rom`。
///
/// # 参数
///
/// - `path`:合集 .txt 文件路径
/// - `sdcard_path`:SD 卡根路径
/// - `platform`:平台代码
///
/// # 返回值
///
/// 存在条目的条目列表。
fn get_collection(path: &str, sdcard_path: &str, platform: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    if let Some(content) = get_file(path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let sd_path = format!("{sdcard_path}{line}");
            if !exists(&sd_path) {
                continue;
            }
            let entry_type = if suffix_match(".pak", &sd_path) {
                EntryType::Pak
            } else {
                EntryType::Rom
            };
            entries.push(entry_new(&sd_path, entry_type, platform));
        }
    }
    entries
}

/// m3u 播放列表 → 碟片条目转换
///
/// 对应 C `getDiscs`（minui.c:799-836）。逐行解析（跳过空行），行内容为
/// m3u 所在目录下的相对文件名 → 拼 base → `exists` 过滤 → 命名为
/// `"Disc {n}"`（n 从 1 递增，**仅计存在行**——C 的 `disc` 计数在 exists
/// 判断之后，minui.c:823-829）。
///
/// # 参数
///
/// - `path`:m3u 文件路径
/// - `platform`:平台代码
///
/// # 返回值
///
/// 存在碟片的 `"Disc N"` 条目列表。
fn get_discs(path: &str, platform: &str) -> Vec<Entry> {
    let mut entries = Vec::new();
    let Some((base, _)) = path.rsplit_once('/') else {
        return entries;
    };
    if let Some(content) = get_file(path) {
        let mut disc = 0;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let disc_path = format!("{base}/{line}");
            if !exists(&disc_path) {
                continue;
            }
            disc += 1;
            let mut entry = entry_new(&disc_path, EntryType::Rom, platform);
            entry.name = format!("Disc {disc}");
            entries.push(entry);
        }
    }
    entries
}

/// 重名条目的唯一标识：`name (emu_tag)`
///
/// 对应 C `getUniqueName`（minui.c:199-212）：显示名 + 空格 + 括号 + 模拟器
/// 标签（`get_emu_name` 的括号内缩写，如 `"Pokemon Red (GB)"`）。
fn get_unique_name(path: &str, name: &str, sdcard_path: &str) -> String {
    let roms_path = get_roms_path(sdcard_path);
    let emu_tag = get_emu_name(path, &roms_path);
    format!("{name} ({emu_tag})")
}

/// 首字符的字母组序号
///
/// 对应 C `getIndexChar`（minui.c:192-197）：`to_ascii_lowercase` 后在 a-z
/// → 1-26（a=1），否则 0。
fn get_index_char(name: &str) -> i32 {
    match name.chars().next() {
        Some(c) => {
            let lower = c.to_ascii_lowercase();
            if lower.is_ascii_lowercase() {
                lower as i32 - 'a' as i32 + 1
            } else {
                0
            }
        }
        None => 0,
    }
}

/// 目录索引：map.txt 重映射 / hide 过滤 / unique 后缀 / alphas 构建
///
/// 对应 C `Directory_index`（minui.c:214-322），三阶段：
///
/// 1. **map.txt 重映射**（C :218-271）：`is_collection`（Collections 前缀）
///    时读 `{Collections}/map.txt`，否则读 `{path}/map.txt`；每行
///    `filename\talias`，条目文件名命中 → 替换 `name`；存在 `hide(name)`
///    命中 → 过滤该条目；发生替换 → 重新排序。Recently Played 与合集
///    目录 SHALL 不建立字母索引（`skip_index`，C :216）
/// 2. **unique 后缀**（C :287-306）：相邻条目 `name` 相等 → 文件名相等者
///    用 `get_unique_name`（含 emu 标签），否则用对方文件名
/// 3. **alphas 构建**（C :308-316）：`skip_index` 之外，`get_index_char`
///    变化时 `alphas.push(i)`，且条目 `alpha = alphas.len() - 1`
///
/// **与 C 的有意偏差**：C 在过滤后还会再遍历一遍重新应用 alias（:273-285，
/// 因 Hash 存活且 map 命中条目未被过滤），Rust 版阶段 1 一次完成——输出
/// 一致，消除重复代码。
///
/// # 参数
///
/// - `dir`:目标目录（entries 就地进行重映射/过滤/索引）
/// - `sdcard_path`:SD 卡根路径
fn directory_index(dir: &mut Directory, sdcard_path: &str) {
    let collections_path = get_collections_path(sdcard_path);
    let is_collection = prefix_match(&collections_path, &dir.path);
    let skip_index = exact_match(&get_faux_recent_path(sdcard_path), &dir.path) || is_collection;

    // ── 阶段 1：map.txt 重映射 + hide 过滤（C :218-271）──
    let map_path = if is_collection {
        format!("{collections_path}/map.txt")
    } else {
        format!("{}/map.txt", dir.path)
    };
    let mut map: HashMap<String, String> = HashMap::new();
    if let Some(content) = get_file(&map_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some((key, value)) = line.split_once('\t') {
                map.insert(key.to_string(), value.to_string());
            }
        }
    }
    if !map.is_empty() {
        let mut resort = false;
        let mut filter = false;
        for entry in &mut dir.entries {
            let filename = entry.path.rsplit('/').next().unwrap_or("").to_string();
            if let Some(alias) = map.get(&filename) {
                entry.name = alias.clone();
                resort = true;
                if !filter && hide(&entry.name) {
                    filter = true;
                }
            }
        }
        if filter {
            dir.entries.retain(|e| !hide(&e.name));
        }
        if resort {
            sort_entries(&mut dir.entries);
        }
    }

    // ── 阶段 2：unique 后缀（C :287-306）──
    for i in 1..dir.entries.len() {
        let prior = dir.entries[i - 1].clone();
        let current = dir.entries[i].clone();
        if !exact_match(&prior.name, &current.name) {
            continue;
        }
        let prior_filename = prior.path.rsplit('/').next().unwrap_or("").to_string();
        let current_filename = current.path.rsplit('/').next().unwrap_or("").to_string();
        if exact_match(&prior_filename, &current_filename) {
            dir.entries[i - 1].unique =
                Some(get_unique_name(&prior.path, &prior.name, sdcard_path));
            dir.entries[i].unique =
                Some(get_unique_name(&current.path, &current.name, sdcard_path));
        } else {
            dir.entries[i - 1].unique = Some(prior_filename);
            dir.entries[i].unique = Some(current_filename);
        }
    }

    // ── 阶段 3：alphas 构建（C :308-316）──
    if !skip_index {
        let mut alpha = -1;
        for (i, entry) in dir.entries.iter_mut().enumerate() {
            let a = get_index_char(&entry.name);
            if a != alpha {
                dir.alphas.push(i);
                alpha = a;
            }
            entry.alpha = dir.alphas.len() - 1;
        }
    }
}

/// SD 卡根目录条目构造
///
/// 对应 C `getRoot`（minui.c:639-753），顺序不可打乱（后步骤依赖前步骤
/// 的结果）：
///
/// 1. `recents::find_recents` 有可用条目 → 插入 Recently Played 伪目录
/// 2. 遍历 Roms 单层：hide 过滤、`has_roms` 通过者 → `Dir` 条目；排序后
///    相邻重名去重（C :661-672——map 重映射前按原名去重）
/// 3. `{Roms}/map.txt` 存在 → 别名重映射并重排序（C :677-715，无 hide
///    过滤——C 注释 "we don't support hidden remaps here"）
/// 4. `has_collections` 为真：步骤 2 有条目 → 插入 Collections 伪目录；
///    无条目 → Collections 下子目录（hide 过滤、排序）并入
/// 5. 全部条目并入结果
/// 6. Tools 目录存在且非 simple_mode → 插入 Tools 条目
///
/// # 参数
///
/// - `sdcard_path`:SD 卡根路径
/// - `platform`:平台代码
/// - `paks_path`:平台 paks 目录
///
/// # 返回值
///
/// 根目录条目列表（Recently Played 伪目录在前）。
fn get_root(sdcard_path: &str, platform: &str, paks_path: &str) -> Vec<Entry> {
    let roms_path = get_roms_path(sdcard_path);
    let collections_path = get_collections_path(sdcard_path);
    let mut root: Vec<Entry> = Vec::new();

    // ── 1. Recently Played 伪目录（C :642）──
    if recents::find_recents(sdcard_path, platform, paks_path).is_some() {
        root.push(entry_new(
            &get_faux_recent_path(sdcard_path),
            EntryType::Dir,
            platform,
        ));
    }

    // ── 2. Roms console 目录（C :644-675）──
    let mut entries: Vec<Entry> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&roms_path) {
        let mut emus: Vec<Entry> = Vec::new();
        for sub in rd.flatten() {
            let name = sub.file_name().to_string_lossy().to_string();
            if hide(&name) {
                continue;
            }
            if !sub.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            if has_roms(&name, sdcard_path, platform, paks_path) {
                emus.push(entry_new(
                    &format!("{roms_path}/{name}"),
                    EntryType::Dir,
                    platform,
                ));
            }
        }
        sort_entries(&mut emus);
        // 相邻重名去重（C :661-672）
        let mut prev: Option<String> = None;
        for entry in emus {
            if let Some(p) = &prev
                && exact_match(p, &entry.name)
            {
                continue;
            }
            prev = Some(entry.name.clone());
            entries.push(entry);
        }
    }

    // ── 3. Roms/map.txt 重映射（C :677-715）──
    let map_path = format!("{roms_path}/map.txt");
    if !entries.is_empty()
        && exists(&map_path)
        && let Some(content) = get_file(&map_path)
    {
        let mut map: HashMap<String, String> = HashMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some((key, value)) = line.split_once('\t') {
                map.insert(key.to_string(), value.to_string());
            }
        }
        let mut resort = false;
        for entry in &mut entries {
            let filename = entry.path.rsplit('/').next().unwrap_or("").to_string();
            if let Some(alias) = map.get(&filename) {
                entry.name = alias.clone();
                resort = true;
            }
        }
        if resort {
            sort_entries(&mut entries);
        }
    }
    // ── 4. Collections（C :717-741）──
    if has_collections(sdcard_path) {
        if !entries.is_empty() {
            root.push(entry_new(&collections_path, EntryType::Dir, platform));
        } else {
            // 无可见系统，提升 collections 到根
            if let Ok(rd) = std::fs::read_dir(&collections_path) {
                let mut collections: Vec<Entry> = Vec::new();
                for sub in rd.flatten() {
                    let name = sub.file_name().to_string_lossy().to_string();
                    if hide(&name) {
                        continue;
                    }
                    collections.push(entry_new(
                        &format!("{collections_path}/{name}"),
                        EntryType::Dir,
                        platform,
                    ));
                }
                sort_entries(&mut collections);
                entries.extend(collections);
            }
        }
    }

    // ── 5. 并入 root（C :743-747）──
    root.extend(entries);

    // ── 6. Tools（C :749-750）──
    let tools_path = get_tools_dir(sdcard_path, platform);
    if exists(&tools_path) && !exists(&get_simple_mode_path(sdcard_path)) {
        root.push(entry_new(&tools_path, EntryType::Dir, platform));
    }

    root
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::paths::CHANGE_DISC_PATH;
    use common::utils::remove_file;
    use std::fs;

    /// 生成测试 SD 卡根目录（正斜杠路径——Windows 的 `std::fs` 接受 `/`，
    /// 且与真实设备上的路径格式一致）
    fn temp_root(name: &str) -> String {
        let base = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        format!("{base}/minui_browser_{}_{name}", std::process::id())
    }

    /// 构造 SD 卡目录结构（Roms + .userdata/shared/.minui）
    fn setup_sdcard(root: &str) {
        fs::create_dir_all(format!("{root}/.userdata/shared/.minui")).unwrap();
        fs::create_dir_all(format!("{root}/Roms")).unwrap();
    }

    /// 安装模拟器（优先候选路径）
    fn install_emu(root: &str, platform: &str, emu: &str) {
        let pak = format!("{root}/Emus/{platform}/{emu}.pak/launch.sh");
        fs::create_dir_all(std::path::Path::new(&pak).parent().unwrap()).unwrap();
        fs::write(&pak, "#!/bin/sh\n").unwrap();
    }

    fn paks_path(root: &str, platform: &str) -> String {
        format!("{root}/.system/{platform}/paks")
    }

    const PLATFORM: &str = "tg5040";

    /// `/tmp/change_disc.txt` 是全局共享文件——所有经 `find_recents` 的测试
    /// 必须串行（browser 内部互斥）且确保文件不存在（防 recents 测试遗留）
    fn lock_tmp() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::test_util::TMP_LOCK.lock().unwrap();
        let _ = remove_file(CHANGE_DISC_PATH);
        guard
    }

    // ── 1. 结构体字段（任务 1.1）──

    #[test]
    fn struct_fields_match_c() {
        let entry = Entry {
            path: "/Roms/x.gb".to_string(),
            name: "x".to_string(),
            unique: None,
            alpha: 0,
            entry_type: EntryType::Rom,
        };
        assert!(entry.unique.is_none());
        assert_eq!(entry.alpha, 0);
        let dir = Directory {
            path: "/Roms".to_string(),
            name: "Roms".to_string(),
            entries: vec![entry],
            selected: 0,
            start: 0,
            end: 1,
            alphas: vec![0],
        };
        assert_eq!(dir.end, 1);
        assert_eq!(dir.alphas, vec![0]);
        assert_eq!(dir.selected, 0);
    }

    // ── 2. has_roms / has_collections（任务 2.1-2.4）──

    #[test]
    fn has_roms_double_condition() {
        let root = temp_root("has_roms");
        setup_sdcard(&root);
        let paks = paks_path(&root, PLATFORM);

        // 模拟器未安装 → false
        let _ = fs::create_dir_all(format!("{root}/Roms/SFC"));
        assert!(!has_roms("SFC", &root, PLATFORM, &paks));

        // 模拟器已装但目录为空 → false
        install_emu(&root, PLATFORM, "GB");
        let gb = format!("{root}/Roms/Game Boy (GB)");
        fs::create_dir_all(&gb).unwrap();
        assert!(!has_roms("Game Boy (GB)", &root, PLATFORM, &paks));

        // 仅隐藏条目 → false
        fs::write(format!("{gb}/.hidden"), "x").unwrap();
        assert!(!has_roms("Game Boy (GB)", &root, PLATFORM, &paks));

        // 正常命中 → true
        fs::write(format!("{gb}/Pokemon Red.gb"), "x").unwrap();
        assert!(has_roms("Game Boy (GB)", &root, PLATFORM, &paks));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn has_collections_boundaries() {
        let root = temp_root("has_collections");
        setup_sdcard(&root);

        // 目录不存在 → false
        assert!(!has_collections(&root));

        // 仅隐藏条目 → false
        let collections = format!("{root}/Collections");
        fs::create_dir_all(format!("{collections}/.hidden")).unwrap();
        assert!(!has_collections(&root));

        // 正常命中 → true
        fs::create_dir_all(format!("{collections}/My Collection")).unwrap();
        assert!(has_collections(&root));

        fs::remove_dir_all(&root).unwrap();
    }

    // ── 3. add_entries / get_entries / 排序（任务 3.1-3.6）──

    #[test]
    fn add_entries_single_level_scan_and_types() {
        let root = temp_root("add_entries");
        setup_sdcard(&root);
        let dir = format!("{root}/Roms/SFC");
        fs::create_dir_all(&dir).unwrap();
        // 目录：Tools.pak → Pak；Sub → Dir
        fs::create_dir_all(format!("{dir}/Tools.pak")).unwrap();
        fs::create_dir_all(format!("{dir}/Sub")).unwrap();
        // 文件：Game.sfc → Rom；.hidden → 跳过；map.txt → 跳过
        fs::write(format!("{dir}/Game.sfc"), "x").unwrap();
        fs::write(format!("{dir}/.hidden"), "x").unwrap();
        fs::write(format!("{dir}/map.txt"), "x").unwrap();

        let mut entries = Vec::new();
        add_entries(&mut entries, &dir, &root, PLATFORM);

        assert_eq!(entries.len(), 3, "隐藏条目与 map.txt 应被过滤");
        let pak = entries
            .iter()
            .find(|e| e.entry_type == EntryType::Pak)
            .unwrap();
        assert_eq!(pak.path, format!("{dir}/Tools.pak"));
        let sub = entries
            .iter()
            .find(|e| e.path == format!("{dir}/Sub"))
            .unwrap();
        assert_eq!(sub.entry_type, EntryType::Dir);
        let rom = entries
            .iter()
            .find(|e| e.entry_type == EntryType::Rom)
            .unwrap();
        assert_eq!(rom.name, "Game", "显示名应去除扩展名");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn add_entries_collections_file_is_dir_type() {
        let root = temp_root("add_entries_coll");
        setup_sdcard(&root);
        let collections = format!("{root}/Collections");
        fs::create_dir_all(&collections).unwrap();
        // Collections 下的文件判 Dir（C :886-888）
        fs::write(format!("{collections}/note.txt"), "x").unwrap();

        let mut entries = Vec::new();
        add_entries(&mut entries, &collections, &root, PLATFORM);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry_type, EntryType::Dir);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn get_entries_console_collation() {
        let root = temp_root("collate");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "GB");
        install_emu(&root, PLATFORM, "USA");
        install_emu(&root, PLATFORM, "GBC");
        fs::create_dir_all(format!("{root}/Roms/Game Boy (GB)")).unwrap();
        fs::create_dir_all(format!("{root}/Roms/Game Boy (USA)")).unwrap();
        fs::create_dir_all(format!("{root}/Roms/Game Boy Color (GBC)")).unwrap();
        fs::write(format!("{root}/Roms/Game Boy (GB)/Pokemon Red.gb"), "x").unwrap();
        fs::write(format!("{root}/Roms/Game Boy (USA)/Blue.gb"), "x").unwrap();
        fs::write(format!("{root}/Roms/Game Boy Color (GBC)/Crystal.gbc"), "x").unwrap();

        // 打开 Game Boy (GB) → collated 前缀 "Roms/Game Boy (" 只命中同家族
        // 目录（Game Boy (GB)/Game Boy (USA)），不命中 Game Boy Color (GBC)
        let entries = get_entries(&format!("{root}/Roms/Game Boy (GB)"), &root, PLATFORM);
        assert_eq!(entries.len(), 2, "同家族目录合并，Game Boy Color 不参与");
        assert!(entries.iter().any(|e| e.name == "Pokemon Red"));
        assert!(entries.iter().any(|e| e.name == "Blue"));
        assert!(!entries.iter().any(|e| e.name == "Crystal"));

        // 非 console 子目录不合并
        let ff = format!("{root}/Roms/SFC/Final Fantasy VII");
        fs::create_dir_all(&ff).unwrap();
        fs::write(format!("{ff}/Disc 1.sfc"), "x").unwrap();
        let entries = get_entries(&ff, &root, PLATFORM);
        assert_eq!(entries.len(), 1, "非 console 目录只扫自身");
        assert_eq!(entries[0].name, "Disc 1");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn sort_entries_case_insensitive() {
        let root = temp_root("sort");
        setup_sdcard(&root);
        let dir = format!("{root}/Roms/SFC");
        fs::create_dir_all(&dir).unwrap();
        fs::write(format!("{dir}/C.sfc"), "x").unwrap();
        fs::write(format!("{dir}/a.sfc"), "x").unwrap();
        fs::write(format!("{dir}/B.sfc"), "x").unwrap();

        let entries = get_entries(&dir, &root, PLATFORM);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["a", "B", "C"], "不区分大小写：a < B < C");

        fs::remove_dir_all(&root).unwrap();
    }

    // ── 4. 转换构造器（任务 4.1-4.6）──

    #[test]
    fn get_recents_filters_and_aliases() {
        let root = temp_root("recents_conv");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "GB");
        fs::create_dir_all(format!("{root}/Roms/Game Boy (GB)")).unwrap();
        fs::write(format!("{root}/Roms/Game Boy (GB)/Pokemon Red.gb"), "x").unwrap();

        let recents = vec![
            Recent {
                path: "/Roms/Game Boy (GB)/Pokemon Red.gb".to_string(),
                alias: Some("Pokemon Red".to_string()),
                available_when_load: true,
            },
            Recent {
                path: "/Roms/SFC/Gone.sfc".to_string(),
                alias: None,
                available_when_load: false,
            },
        ];
        let entries = get_recents(&recents, &root, PLATFORM);
        assert_eq!(entries.len(), 1, "不可用条目应被过滤");
        assert_eq!(entries[0].name, "Pokemon Red", "alias 覆盖显示名");
        assert_eq!(entries[0].entry_type, EntryType::Rom);
        assert_eq!(
            entries[0].path,
            format!("{root}/Roms/Game Boy (GB)/Pokemon Red.gb")
        );

        // .pak 条目判 Pak
        let recents = vec![Recent {
            path: "/Roms/Tools.pak".to_string(),
            alias: None,
            available_when_load: true,
        }];
        let entries = get_recents(&recents, &root, PLATFORM);
        assert_eq!(entries[0].entry_type, EntryType::Pak);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn get_collection_parses_and_filters() {
        let root = temp_root("collection");
        setup_sdcard(&root);
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        fs::write(format!("{root}/Roms/SFC/A.sfc"), "x").unwrap();
        let collection_file = format!("{root}/Collections/My.txt");
        fs::create_dir_all(format!("{root}/Collections")).unwrap();
        fs::write(&collection_file, "/Roms/SFC/A.sfc\n\n/Roms/SFC/Gone.sfc\n").unwrap();

        let entries = get_collection(&collection_file, &root, PLATFORM);
        assert_eq!(entries.len(), 1, "空行与不存在条目应被过滤");
        assert_eq!(entries[0].name, "A");
        assert_eq!(entries[0].path, format!("{root}/Roms/SFC/A.sfc"));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn get_discs_disc_n_naming() {
        let root = temp_root("discs");
        setup_sdcard(&root);
        let game_dir = format!("{root}/Roms/SFC/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(format!("{game_dir}/Disc 1.sfc"), "x").unwrap();
        let m3u = format!("{game_dir}/Final Fantasy VII.m3u");
        fs::write(&m3u, "Disc 1.sfc\n\nDisc 2.sfc\n").unwrap();

        let entries = get_discs(&m3u, PLATFORM);
        assert_eq!(entries.len(), 1, "空行与不存在的碟片应跳过");
        assert_eq!(entries[0].name, "Disc 1");
        assert_eq!(entries[0].path, format!("{game_dir}/Disc 1.sfc"));
        assert_eq!(entries[0].entry_type, EntryType::Rom);

        fs::remove_dir_all(&root).unwrap();
    }

    // ── 5. directory_index（任务 5.1-5.7）──

    /// 构造未索引的 Directory（path + 由 entry_new 生成的条目）
    fn make_dir(path: &str, names: &[&str]) -> Directory {
        let entries: Vec<Entry> = names
            .iter()
            .map(|n| entry_new(&format!("{path}/{n}"), EntryType::Rom, PLATFORM))
            .collect();
        let end = entries.len();
        Directory {
            path: path.to_string(),
            name: get_display_name(path, PLATFORM),
            entries,
            selected: 0,
            start: 0,
            end,
            alphas: Vec::new(),
        }
    }

    #[test]
    fn index_map_remap_two_locations() {
        let root = temp_root("map_remap");
        setup_sdcard(&root);
        let dir_path = format!("{root}/Roms/SFC");
        fs::create_dir_all(&dir_path).unwrap();
        fs::write(format!("{dir_path}/map.txt"), "Game.sfc\tFinal Fantasy\n").unwrap();
        fs::write(format!("{dir_path}/Game.sfc"), "x").unwrap();

        // 普通目录读取 {path}/map.txt
        let mut dir = make_dir(&dir_path, &["Game.sfc"]);
        directory_index(&mut dir, &root);
        assert_eq!(dir.entries[0].name, "Final Fantasy");

        // 合集目录读取 {Collections}/map.txt
        let collections = format!("{root}/Collections");
        fs::create_dir_all(&collections).unwrap();
        fs::write(
            format!("{collections}/map.txt"),
            "Alpha.txt\tMy Collection\n",
        )
        .unwrap();
        let coll_dir = format!("{collections}/Alpha.txt");
        let mut dir = make_dir(&coll_dir, &["Alpha.txt"]);
        directory_index(&mut dir, &root);
        assert_eq!(dir.entries[0].name, "My Collection");
        // 合集 skip_index：不建字母索引
        assert!(dir.alphas.is_empty(), "合集目录不建字母索引");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn index_map_hide_filter_and_resort() {
        let root = temp_root("map_hide");
        setup_sdcard(&root);
        let dir_path = format!("{root}/Roms/SFC");
        fs::create_dir_all(&dir_path).unwrap();
        // Secret.sfc → .hidden（hide 命中）；Zebra.sfc → Apple（resort）
        fs::write(
            format!("{dir_path}/map.txt"),
            "Secret.sfc\t.hidden\nZebra.sfc\tApple\n",
        )
        .unwrap();
        fs::write(format!("{dir_path}/Secret.sfc"), "x").unwrap();
        fs::write(format!("{dir_path}/Zebra.sfc"), "x").unwrap();
        fs::write(format!("{dir_path}/Middle.sfc"), "x").unwrap();

        let mut dir = make_dir(&dir_path, &["Secret.sfc", "Zebra.sfc", "Middle.sfc"]);
        directory_index(&mut dir, &root);
        let names: Vec<&str> = dir.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Apple", "Middle"],
            "Secret 被过滤、Zebra 重映射后排序"
        );
        assert!(
            dir.entries
                .iter()
                .all(|e| e.path != format!("{dir_path}/Secret.sfc"))
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn index_unique_suffixes() {
        let root = temp_root("unique");
        setup_sdcard(&root);
        install_emu(&root, PLATFORM, "GB");
        install_emu(&root, PLATFORM, "USA");
        fs::create_dir_all(format!("{root}/Roms/Game Boy (GB)")).unwrap();
        fs::create_dir_all(format!("{root}/Roms/Game Boy (USA)")).unwrap();
        // 同名同文件名（同家族不同 console 目录，经 collate 合并）→ emu 标签 unique
        fs::write(format!("{root}/Roms/Game Boy (GB)/Pokemon Red.gb"), "x").unwrap();
        fs::write(format!("{root}/Roms/Game Boy (USA)/Pokemon Red.gb"), "x").unwrap();

        let dir = Directory::new(
            &format!("{root}/Roms/Game Boy (GB)"),
            0,
            &root,
            PLATFORM,
            &paks_path(&root, PLATFORM),
        );
        assert_eq!(dir.entries.len(), 2);
        let uniques: Vec<Option<String>> = dir.entries.iter().map(|e| e.unique.clone()).collect();
        assert!(uniques.iter().all(|u| u.is_some()));
        let mut tags: Vec<String> = uniques.into_iter().flatten().collect();
        tags.sort();
        assert_eq!(tags, vec!["Pokemon Red (GB)", "Pokemon Red (USA)"]);

        // 同名不同文件名 → 文件名 unique
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        fs::write(format!("{root}/Roms/SFC/Game (USA).sfc"), "x").unwrap();
        let mut dir = make_dir(&format!("{root}/Roms/SFC"), &["Game.sfc", "Game (USA).sfc"]);
        directory_index(&mut dir, &root);
        let uniques: Vec<Option<String>> = dir.entries.iter().map(|e| e.unique.clone()).collect();
        assert_eq!(
            uniques,
            vec![
                Some("Game.sfc".to_string()),
                Some("Game (USA).sfc".to_string())
            ]
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn index_alphas_building() {
        let root = temp_root("alphas");
        setup_sdcard(&root);
        let dir_path = format!("{root}/Roms/SFC");
        fs::create_dir_all(&dir_path).unwrap();

        // Z / M / 非字母 → 三组
        let mut dir = make_dir(&dir_path, &["Zelda.sfc", "Mario.sfc", "007.sfc"]);
        directory_index(&mut dir, &root);
        assert_eq!(dir.alphas, vec![0, 1, 2]);
        assert_eq!(
            dir.entries.iter().map(|e| e.alpha).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );

        // 相邻同字母 → 一组
        let mut dir = make_dir(&dir_path, &["Ape.sfc", "Ant.sfc", "Zoo.sfc"]);
        directory_index(&mut dir, &root);
        assert_eq!(dir.alphas, vec![0, 2]);
        assert_eq!(dir.entries[0].alpha, 0);
        assert_eq!(dir.entries[1].alpha, 0);
        assert_eq!(dir.entries[2].alpha, 1);

        // Recently Played skip_index
        let mut dir = make_dir(&get_faux_recent_path(&root), &["Ape.gb", "Zoo.gb"]);
        directory_index(&mut dir, &root);
        assert!(dir.alphas.is_empty(), "Recently Played 不建字母索引");
        assert_eq!(dir.entries[0].alpha, 0, "alpha 保持初始值");

        fs::remove_dir_all(&root).unwrap();
    }

    // ── 6. get_root（任务 6.1-6.4）──

    #[test]
    fn root_recents_pseudo_dir() {
        let root = temp_root("root_recents");
        setup_sdcard(&root);
        let paks = paks_path(&root, PLATFORM);

        // 无最近游玩 → 无伪目录
        let root_entries = get_root(&root, PLATFORM, &paks);
        assert!(
            root_entries
                .iter()
                .all(|e| e.path != get_faux_recent_path(&root)),
            "无可用最近游玩时不应有伪目录"
        );

        // 有最近游玩 → 伪目录在首位
        install_emu(&root, PLATFORM, "GB");
        fs::create_dir_all(format!("{root}/Roms/Game Boy (GB)")).unwrap();
        fs::write(format!("{root}/Roms/Game Boy (GB)/Pokemon Red.gb"), "x").unwrap();
        fs::write(
            format!("{root}/.userdata/shared/.minui/recent.txt"),
            "/Roms/Game Boy (GB)/Pokemon Red.gb\n",
        )
        .unwrap();
        let _guard = lock_tmp();
        let root_entries = get_root(&root, PLATFORM, &paks);
        assert_eq!(
            root_entries[0].path,
            get_faux_recent_path(&root),
            "Recently Played 伪目录应在首位"
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn root_roms_filter_dedup_and_map() {
        let root = temp_root("root_roms");
        setup_sdcard(&root);
        let paks = paks_path(&root, PLATFORM);

        // 无模拟器的 SFC 被过滤
        fs::create_dir_all(format!("{root}/Roms/SFC")).unwrap();
        fs::write(format!("{root}/Roms/SFC/Game.sfc"), "x").unwrap();
        // 同名 console 目录去重（Dup 与 Dup (X) 显示名相同）
        install_emu(&root, PLATFORM, "Dup");
        install_emu(&root, PLATFORM, "X");
        fs::create_dir_all(format!("{root}/Roms/Dup")).unwrap();
        fs::write(format!("{root}/Roms/Dup/A.gb"), "x").unwrap();
        fs::create_dir_all(format!("{root}/Roms/Dup (X)")).unwrap();
        fs::write(format!("{root}/Roms/Dup (X)/B.gb"), "x").unwrap();
        // Roms/map.txt 重映射
        fs::write(format!("{root}/Roms/map.txt"), "Dup\tRenamed\n").unwrap();

        let root_entries = get_root(&root, PLATFORM, &paks);
        let names: Vec<&str> = root_entries.iter().map(|e| e.name.as_str()).collect();
        assert!(!names.contains(&"SFC"), "无模拟器的 console 目录应被过滤");
        assert!(names.contains(&"Renamed"), "map.txt 重映射生效");
        let dup_count = names.iter().filter(|n| **n == "Renamed").count();
        assert_eq!(dup_count, 1, "相邻重名应去重");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn root_collections_and_tools() {
        let root = temp_root("root_coll_tools");
        setup_sdcard(&root);
        let paks = paks_path(&root, PLATFORM);
        let collections = format!("{root}/Collections");
        let tools = format!("{root}/Tools/{PLATFORM}");

        // 无系统：Collections 提升到根
        fs::create_dir_all(format!("{collections}/My Collection")).unwrap();
        fs::create_dir_all(&tools).unwrap();
        let root_entries = get_root(&root, PLATFORM, &paks);
        assert!(
            root_entries.iter().all(|e| e.path != collections),
            "无可见系统时不应有 Collections 伪目录"
        );
        assert!(
            root_entries
                .iter()
                .any(|e| e.path == format!("{collections}/My Collection")),
            "Collections 子目录应提升到根"
        );
        assert!(root_entries.iter().any(|e| e.path == tools), "Tools 应显示");

        // 有系统：Collections 伪目录 + simple_mode 隐藏 Tools
        install_emu(&root, PLATFORM, "GB");
        fs::create_dir_all(format!("{root}/Roms/Game Boy (GB)")).unwrap();
        fs::write(format!("{root}/Roms/Game Boy (GB)/A.gb"), "x").unwrap();
        fs::write(format!("{root}/.userdata/shared/enable-simple-mode"), "").unwrap();
        let root_entries = get_root(&root, PLATFORM, &paks);
        assert!(
            root_entries.iter().any(|e| e.path == collections),
            "有可见系统时应显示 Collections 伪目录"
        );
        assert!(
            root_entries.iter().all(|e| e.path != tools),
            "simple_mode 下 Tools 应隐藏"
        );

        fs::remove_dir_all(&root).unwrap();
    }

    // ── 7. Directory::new 分发器（任务 7.1）──

    #[test]
    fn directory_new_dispatch_and_selected() {
        let root = temp_root("dispatch");
        setup_sdcard(&root);
        let paks = paks_path(&root, PLATFORM);
        install_emu(&root, PLATFORM, "GB");
        fs::create_dir_all(format!("{root}/Roms/Game Boy (GB)")).unwrap();
        fs::write(format!("{root}/Roms/Game Boy (GB)/Pokemon Red.gb"), "x").unwrap();
        fs::create_dir_all(format!("{root}/Tools/{PLATFORM}")).unwrap();

        // ① SDCARD 根目录
        let dir = Directory::new(&root, 0, &root, PLATFORM, &paks);
        assert!(dir.name.ends_with("dispatch"), "根目录显示名 = 目录名");
        assert!(
            dir.entries
                .iter()
                .any(|e| e.path == format!("{root}/Tools/{PLATFORM}"))
        );

        // ② Recently Played
        fs::write(
            format!("{root}/.userdata/shared/.minui/recent.txt"),
            "/Roms/Game Boy (GB)/Pokemon Red.gb\n",
        )
        .unwrap();
        let _guard = lock_tmp();
        let dir = Directory::new(&get_faux_recent_path(&root), 0, &root, PLATFORM, &paks);
        assert_eq!(dir.entries.len(), 1);
        assert_eq!(dir.entries[0].name, "Pokemon Red");
        drop(_guard);

        // ③ Collections .txt
        fs::create_dir_all(format!("{root}/Collections")).unwrap();
        fs::write(
            format!("{root}/Collections/My.txt"),
            "/Roms/Game Boy (GB)/Pokemon Red.gb\n",
        )
        .unwrap();
        let dir = Directory::new(
            &format!("{root}/Collections/My.txt"),
            0,
            &root,
            PLATFORM,
            &paks,
        );
        assert_eq!(dir.entries.len(), 1);
        assert_eq!(dir.name, "My");

        // ④ m3u
        let m3u = format!("{root}/Roms/Game Boy (GB)/Multi.m3u");
        fs::write(&m3u, "Pokemon Red.gb\n").unwrap();
        let dir = Directory::new(&m3u, 0, &root, PLATFORM, &paks);
        assert_eq!(dir.entries.len(), 1);
        assert_eq!(dir.entries[0].name, "Disc 1");

        // ⑤ 普通目录 + selected 透传（含 m3u 文件本身与 ROM → 2 条）
        let dir = Directory::new(
            &format!("{root}/Roms/Game Boy (GB)"),
            3,
            &root,
            PLATFORM,
            &paks,
        );
        assert_eq!(dir.selected, 3);
        assert_eq!(dir.entries.len(), 2);

        fs::remove_dir_all(&root).unwrap();
    }
}
