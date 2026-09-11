//! 文件 I/O 和字符串工具
//!
//! 本模块提供 minui 和 minarch 共用的工具函数，涵盖 SD 卡文件操作
//! 和 ROM 文件名处理。
//!
//! ## 设计
//!
//! 所有函数使用 Rust 标准库（`std::fs`、`std::path`）实现，
//! 不依赖任何第三方 crate。这符合 common 零依赖的原则。
//!
//! ## 与原 C 代码的对比
//!
//! 原版 `utils.c` 使用 POSIX API（`fopen`、`opendir`、`readdir`）
//! 和 C 字符串操作（`strcpy`、`strrchr`、`sprintf`）。栈上 `char[256]`
//! 缓冲区遍布各处，容易发生缓冲区溢出。Rust 版使用 `String` 和 `Path`，
//! 编译期保证无缓冲区溢出。
//!

use std::path::Path;

// ── 文件 I/O ────────────────────────────────────

/// 检查路径是否存在（文件或目录）
///
/// 对应原版 C `exists()`。
pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// 读取文件全部内容到 `String`
///
/// 对应原版 C `getFile(path, buffer, buffer_size)`。
/// 返回 `None` 如果文件不存在或不是合法 UTF-8。
pub fn get_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// 将字符串写入文件（覆盖）
///
/// 对应原版 C `putFile(path, contents)`。
pub fn put_file(path: &str, contents: &str) -> std::io::Result<()> {
    std::fs::write(path, contents)
}

/// 将整数写入文件（十进制文本）
///
/// 对应原版 C `putInt()`。
pub fn put_int(path: &str, value: i32) -> std::io::Result<()> {
    put_file(path, &value.to_string())
}

/// 从文件读取整数值
///
/// 对应原版 C `getInt()`。
/// 返回 `None` 如果文件不存在或内容不是合法整数。
pub fn get_int(path: &str) -> Option<i32> {
    get_file(path).and_then(|s| s.trim().parse().ok())
}

/// 创建空文件（类似 `touch`）
///
/// 对应原版 C `touch()`。
pub fn touch(path: &str) -> std::io::Result<()> {
    std::fs::File::create(path)?;
    Ok(())
}

/// 删除指定文件
///
/// `std::fs::remove_file` 的薄包装，错误类型直接透传。
///
/// 对应原版 C `unlink()`（`minui.c:538` 删换碟请求文件、
/// `minui.c:1040` 删自动续玩标记）。消费方：minui 的
/// `launch.rs`（autoResume）与 `recents.rs`（hasRecents 消费
/// 换碟请求后清理）——两个模块共用，故置于 common。
///
/// # 参数
///
/// - `path`:要删除的文件路径
///
/// # 返回值
///
/// `Ok(())` 删除成功；`Err` 删除失败（常见错误类型：
/// `NotFound`——路径不存在，语义与 `std::fs::remove_file` 一致）。
pub fn remove_file(path: &str) -> std::io::Result<()> {
    std::fs::remove_file(path)
}

// ── 字符串匹配 ──────────────────────────────────

/// 精确匹配
///
/// 大小写敏感（匹配 C `exactMatch()` 的 `strncmp`）——与其它三个匹配函数
/// 的大小写不敏感语义刻意不同。
///
/// 对应原版 C `exactMatch()`。
pub fn exact_match(a: &str, b: &str) -> bool {
    a == b
}

/// `str` 是否以 `pre` 开头
///
/// ASCII 大小写不敏感（匹配 C `prefixMatch()` 的 `strncasecmp`——ASCII-only
/// 折叠，不是 Unicode 折叠，故不用 `to_lowercase`）。`pre` 为空串时恒为
/// `true`（`strncasecmp("", s, 0)==0` 语义）。
///
/// 对应原版 C `prefixMatch()`。
pub fn prefix_match(pre: &str, s: &str) -> bool {
    s.get(..pre.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(pre))
}

/// `str` 是否以 `suf` 结尾
///
/// ASCII 大小写不敏感（匹配 C `suffixMatch()` 的 `strncasecmp`）。`suf`
/// 长于 `s` 时返回 `false`（对应 C 的 `offset>=0` 检查）。`suf` 为空串时
/// 恒为 `true`。
///
/// 对应原版 C `suffixMatch()`。
pub fn suffix_match(suf: &str, s: &str) -> bool {
    s.get(s.len().saturating_sub(suf.len())..)
        .is_some_and(|p| p.eq_ignore_ascii_case(suf))
}

/// `haystack` 是否包含 `needle`
///
/// ASCII 大小写不敏感（匹配 C `containsString()` 的 `strcasestr`）。
/// `needle` 为空串时恒为 `true`（`strcasestr(h, "")` 非空语义）。
///
/// 对应原版 C `containsString()`。
pub fn contains_string(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

// ── 文件名处理 ──────────────────────────────────

/// 判断文件名是否应隐藏
///
/// 隐藏条件（匹配原 C `utils.c:32-33` 的 `hide()`）：
/// 1. 以 `.` 开头（Unix 隐藏文件约定）
/// 2. 以 `.disabled` 结尾（ASCII 大小写不敏感，复用 `suffix_match`——结构对齐
///    C `hide()` 调用 `suffixMatch(".disabled", ...)`）
/// 3. 精确匹配 `"map.txt"`
///
/// 对应原版 C `hide()`。
pub fn hide(filename: &str) -> bool {
    filename.starts_with('.') || suffix_match(".disabled", filename) || filename == "map.txt"
}

/// 获取文件的显示名称（去除扩展名和地区/版本标记）
///
/// 匹配原 C `utils.c:36-73` 的 `getDisplayName()` 六步管线：
/// 1. 若路径以 `"/{platform}"` 结尾（ASCII 大小写不敏感，对齐 C
///    `suffixMatch("/" PLATFORM, ...)` 的行为），剥离该末尾段（隐藏 Tools
///    路径中的平台目录）
/// 2. 提取最后一个 `'/'` 之后的文件名
/// 3. 循环去除末尾扩展名（1-4 字符的扩展名，含点号 2-5 字符）
/// 4. 循环去除末尾 `(...)` 和 `[...]` 标记
/// 5. 若结果为空字符串，回退到步骤 4 之前的状态
/// 6. 去除末尾空白字符
///
/// # 参数
///
/// - `path`:输入路径
/// - `platform`:平台代码（如 `"tg5040"`，来自 `Platform::PLATFORM`）——
///   用于剥离末尾平台段；函数 SHALL NOT 引用任何 crate 内部路径常量
///
/// # 返回值
///
/// 处理后的显示名称。
///
/// 对应原版 C `getDisplayName(in_name, out_name)`。
pub fn get_display_name(path: &str, platform: &str) -> String {
    let mut work = path.to_string();

    // ── 步骤 1：剥离末尾平台段（大小写不敏感，对齐 C `suffixMatch("/" PLATFORM, ...)`）──
    let platform_suffix = format!("/{platform}");
    if suffix_match(&platform_suffix, &work) {
        let new_len = work.len() - platform_suffix.len();
        work.truncate(new_len);
    }

    // ── 步骤 2：提取文件名 ──
    if let Some(last_slash) = work.rfind('/') {
        work = work[last_slash + 1..].to_string();
    }

    // ── 步骤 3：循环去除末尾扩展名（1-4 字符的扩展名）──
    while let Some(dot) = work.rfind('.') {
        let ext_len = work.len() - dot - 1; // 点号之后的字符数
        if (1..=4).contains(&ext_len) {
            work.truncate(dot);
        } else {
            break; // 扩展名长度不符合 1-4 范围
        }
    }

    // ── 步骤 4：循环去除末尾括号标记 ──
    let before_parens = work.clone();
    loop {
        let paren = work.rfind('(');
        let bracket = work.rfind('[');
        let pos = match (paren, bracket) {
            (Some(p), Some(b)) => Some(p.max(b)),
            (Some(p), None) => Some(p),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        match pos {
            Some(0) => break, // 括号在第一位，不截断（避免清空整个名称）
            Some(p) => {
                work.truncate(p);
                // 继续循环，找更前面的括号
            }
            None => break,
        }
    }

    // ── 步骤 5：空名回退 ──
    if work.is_empty() {
        work = before_parens;
    }

    // ── 步骤 6：去除末尾空白字符 ──
    let trimmed_end = work.trim_end().len();
    work.truncate(trimmed_end);

    work
}

/// 从 ROM 路径推断模拟器名称（即 `.pak` 标签）
///
/// 匹配原 C `utils.c:74-102` 的 `getEmuName()`：
/// 1. 若路径以 `roms_path` 开头（ASCII 大小写不敏感，对齐 C
///    `prefixMatch(ROMS_PATH, ...)` 的行为），提取其后的第一级目录名（模拟器文件夹名）
/// 2. 在目录名中查找最后一个 `(...)`，提取括号内内容作为模拟器缩写
/// 3. 若无括号，返回目录名本身
///
/// # 参数
///
/// - `path`:ROM 文件路径
/// - `roms_path`:ROM 目录路径（如 `"/mnt/SDCARD/Roms"`，来自
///   `common::paths::get_roms_path(P::SDCARD_PATH)`）——函数 SHALL NOT
///   引用任何 crate 内部路径常量
///
/// # 返回值
///
/// 模拟器缩写名称（即 `.pak` 标签）。
///
/// 对应原版 C `getEmuName(in_name, out_name)`。
/// 解析 `/Roms/Game Boy (GB)/game.gb` → `"GB"`。
pub fn get_emu_name(path: &str, roms_path: &str) -> String {
    let name = if prefix_match(roms_path, path) {
        // 前缀匹配成功（ASCII 大小写不敏感，对齐 C `prefixMatch(ROMS_PATH, ...)`）。
        // prefix_match 已保证 0..roms_path.len() 在合法字符边界上，切片安全。
        let after_roms = &path[roms_path.len()..];
        // 提取 roms_path 后的第一级目录名
        let trimmed = after_roms.trim_start_matches('/');
        // 取第一级目录（到下一个 '/' 为止）
        match trimmed.find('/') {
            Some(pos) => &trimmed[..pos],
            None => trimmed,
        }
    } else {
        // 不以 roms_path 开头，直接在原始路径中查找
        path
    };

    // 从末尾括号中提取模拟器缩写
    if let Some(paren_start) = name.rfind('(') {
        let after_paren = &name[paren_start + 1..];
        if let Some(paren_end) = after_paren.find(')') {
            return after_paren[..paren_end].to_string();
        }
    }

    // 无括号，返回目录名/文件名本身
    name.to_string()
}

/// 根据模拟器名称构造 `.pak/launch.sh` 的完整路径
///
/// 匹配原 C `utils.c:103-107` 的 `getEmuPath()` 双候选回退：
/// 1. 优先：`{sdcard_path}/Emus/{platform}/{emu_name}.pak/launch.sh`
/// 2. 回退：`{paks_path}/Emus/{emu_name}.pak/launch.sh`（优先路径不存在时）
///
/// 注意：不保证返回的路径一定存在（即使回退路径也不存在时仍返回回退路径，匹配 C 行为）。
///
/// # 参数
///
/// - `emu_name`:模拟器名称（`.pak` 标签）
/// - `sdcard_path`:SD 卡根路径（来自 `Platform::SDCARD_PATH`）
/// - `platform`:平台代码（来自 `Platform::PLATFORM`）
/// - `paks_path`:平台 paks 目录（来自 `common::paths::get_paks_path`）——
///   函数 SHALL NOT 引用任何 crate 内部路径常量
///
/// # 返回值
///
/// `.pak/launch.sh` 完整路径。
///
/// 对应原版 C `getEmuPath(emu_name, pak_path)`。
pub fn get_emu_path(emu_name: &str, sdcard_path: &str, platform: &str, paks_path: &str) -> String {
    let primary = format!("{sdcard_path}/Emus/{platform}/{emu_name}.pak/launch.sh");
    if exists(&primary) {
        return primary;
    }
    format!("{paks_path}/Emus/{emu_name}.pak/launch.sh")
}

/// 剥离文件名开头的排序前缀
///
/// 匹配原 C `utils.c:123-137` 的 `trimSortingMeta()`：
/// 跳过开头连续数字 → 期望紧跟 `')'` → 跳过 `')'` → 跳过后续空白。
/// 若格式不匹配（数字后不是 `')'`）则不做任何修改。
///
/// 对应原版 C `trimSortingMeta()`。
pub fn trim_sorting_meta(name: &mut String) {
    let chars: Vec<char> = name.chars().collect();
    let mut pos = 0;

    // 跳过开头连续数字
    while pos < chars.len() && chars[pos].is_ascii_digit() {
        pos += 1;
    }

    // 期望紧跟 ')'
    if pos >= chars.len() || chars[pos] != ')' {
        return; // 格式不匹配，不做修改
    }

    // 跳过 ')'
    pos += 1;

    // 跳过后续空白
    while pos < chars.len() && chars[pos].is_ascii_whitespace() {
        pos += 1;
    }

    let new_str: String = chars[pos..].iter().collect();
    *name = new_str;
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 文件 I/O ─────────────────────────────────

    /// 生成测试用临时文件路径（含进程 id，避免并行测试冲突）
    fn temp_path(name: &str) -> String {
        std::env::temp_dir()
            .join(format!("minui_test_{}_{name}", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn remove_file_existing() {
        let path = temp_path("remove_existing.txt");
        std::fs::write(&path, "x").unwrap();
        let result = remove_file(&path);
        assert!(result.is_ok());
        assert!(!std::path::Path::new(&path).exists());
    }

    #[test]
    fn remove_file_missing_returns_not_found() {
        let path = temp_path("remove_missing.txt");
        let _ = std::fs::remove_file(&path); // 确保不存在
        let err = remove_file(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }

    // ── hide ─────────────────────────────────────

    #[test]
    fn hide_dotfile() {
        assert!(hide(".hidden"));
    }

    #[test]
    fn hide_disabled_suffix_lowercase() {
        assert!(hide("core.disabled"));
    }

    #[test]
    fn hide_disabled_suffix_uppercase() {
        assert!(hide("core.DISABLED"));
    }

    #[test]
    fn hide_map_txt() {
        assert!(hide("map.txt"));
    }

    #[test]
    fn hide_normal_file() {
        assert!(!hide("game.gb"));
    }

    #[test]
    fn hide_empty_string() {
        assert!(!hide(""));
    }

    // ── trim_sorting_meta ────────────────────────

    #[test]
    fn trim_sorting_standard_prefix() {
        let mut s = "01) Pokemon Red".to_string();
        trim_sorting_meta(&mut s);
        assert_eq!(s, "Pokemon Red");
    }

    #[test]
    fn trim_sorting_multi_digit_prefix() {
        let mut s = "123) Game".to_string();
        trim_sorting_meta(&mut s);
        assert_eq!(s, "Game");
    }

    #[test]
    fn trim_sorting_no_paren_unchanged() {
        let mut s = "01 - Game".to_string();
        trim_sorting_meta(&mut s);
        assert_eq!(s, "01 - Game");
    }

    #[test]
    fn trim_sorting_no_digits_unchanged() {
        let mut s = "Pokemon Red".to_string();
        trim_sorting_meta(&mut s);
        assert_eq!(s, "Pokemon Red");
    }

    #[test]
    fn trim_sorting_empty_string() {
        let mut s = String::new();
        trim_sorting_meta(&mut s);
        assert_eq!(s, "");
    }

    #[test]
    fn trim_sorting_digits_no_paren() {
        let mut s = "01 Game".to_string();
        trim_sorting_meta(&mut s);
        assert_eq!(s, "01 Game");
    }

    // ── get_emu_name ─────────────────────────────

    #[test]
    fn get_emu_name_standard_roms_path() {
        let result = get_emu_name(
            "/mnt/SDCARD/Roms/Game Boy (GB)/Pokemon Red.gb",
            "/mnt/SDCARD/Roms",
        );
        assert_eq!(result, "GB");
    }

    #[test]
    fn get_emu_name_no_parens_returns_folder() {
        let result = get_emu_name(
            "/mnt/SDCARD/Roms/SFC/Legend of Zelda.sfc",
            "/mnt/SDCARD/Roms",
        );
        assert_eq!(result, "SFC");
    }

    #[test]
    fn get_emu_name_non_roms_path_with_parens() {
        let result = get_emu_name("/other (NES)/game.nes", "/mnt/SDCARD/Roms");
        assert_eq!(result, "NES");
    }

    #[test]
    fn get_emu_name_roms_prefix_case_variant() {
        let result = get_emu_name(
            "/MNT/SDCARD/ROMS/SFC/Legend of Zelda.sfc",
            "/mnt/SDCARD/Roms",
        );
        assert_eq!(result, "SFC");
    }

    // ── get_emu_path ─────────────────────────────

    #[test]
    fn get_emu_path_format() {
        // 测试回退路径格式（优先路径在测试环境通常不存在）
        let result = get_emu_path(
            "GB",
            "/mnt/SDCARD",
            "tg5040",
            "/mnt/SDCARD/.system/tg5040/paks",
        );
        assert!(result.ends_with("/Emus/GB.pak/launch.sh"));
        assert!(result.starts_with("/mnt/SDCARD"));
    }

    // ── get_display_name ─────────────────────────

    #[test]
    fn display_name_standard_rom_file() {
        let result = get_display_name("/mnt/SDCARD/Roms/Game Boy (GB)/Pokemon Red.gb", "tg5040");
        assert_eq!(result, "Pokemon Red");
    }

    #[test]
    fn display_name_single_extension() {
        let result = get_display_name("/path/to/game.gb", "tg5040");
        assert_eq!(result, "game");
    }

    #[test]
    fn display_name_multi_extension() {
        let result = get_display_name("/path/to/game.p8.png", "tg5040");
        assert_eq!(result, "game");
    }

    #[test]
    fn display_name_paren_markers() {
        let result = get_display_name("/path/to/Game (USA) (GB).gba", "tg5040");
        assert_eq!(result, "Game");
    }

    #[test]
    fn display_name_trailing_whitespace_after_paren() {
        let result = get_display_name("/path/to/Game (USA) .nes", "tg5040");
        assert_eq!(result, "Game");
    }

    #[test]
    fn display_name_tools_platform_dir() {
        let result = get_display_name("/mnt/SDCARD/Tools/tg5040/sometool.pak", "tg5040");
        assert_eq!(result, "sometool");
    }

    #[test]
    fn display_name_tools_platform_dir_case_variant() {
        let result = get_display_name("/mnt/SDCARD/Tools/TG5040", "tg5040");
        assert_eq!(result, "Tools");
    }

    #[test]
    fn display_name_empty_fallback() {
        let result = get_display_name("/path/to/(GB).gba", "tg5040");
        assert_eq!(result, "(GB)");
    }

    #[test]
    fn display_name_just_filename() {
        let result = get_display_name("game.gb", "tg5040");
        assert_eq!(result, "game");
    }

    // ── 字符串匹配大小写语义 ────────────────────
    // 对齐 C utils.c:15-30：prefixMatch/suffixMatch/containsString 用
    // strncasecmp/strcasestr（ASCII 大小写不敏感），exactMatch 用 strncmp（敏感）。

    #[test]
    fn prefix_match_case_insensitive() {
        assert!(prefix_match("ABC", "abcxyz"));
        assert!(prefix_match(
            "/mnt/SDCARD/ROMS",
            "/mnt/SDCARD/roms/Game Boy (GB)/game.gb"
        ));
    }

    #[test]
    fn prefix_match_mismatch_at_position_zero() {
        assert!(!prefix_match("abc", "xabc"));
    }

    #[test]
    fn prefix_match_pre_longer_than_string() {
        assert!(!prefix_match("abcd", "abc"));
    }

    #[test]
    fn prefix_match_empty_prefix() {
        assert!(prefix_match("", "anything"));
    }

    #[test]
    fn suffix_match_case_insensitive() {
        assert!(suffix_match(".DISABLED", "core.disabled"));
        assert!(suffix_match(".disabled", "core.DISABLED"));
        assert!(suffix_match("ABC", "xyzabc"));
    }

    #[test]
    fn suffix_match_no_match() {
        assert!(!suffix_match(".disabled", "core.disabledx"));
    }

    #[test]
    fn suffix_match_longer_than_string() {
        assert!(!suffix_match("very.long.suffix", "short"));
    }

    #[test]
    fn exact_match_case_sensitive() {
        assert!(!exact_match("Game", "GAME"));
        assert!(exact_match("Game", "Game"));
    }

    #[test]
    fn contains_string_case_insensitive() {
        assert!(contains_string("Game Boy (GB)", "boy"));
        assert!(!contains_string("game", "x"));
    }

    #[test]
    fn contains_string_empty_needle() {
        assert!(contains_string("game", ""));
    }
}
