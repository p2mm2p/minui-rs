//! # 工具函数
//!
//! 对应原 C 代码 `utils.c` / `utils.h` 中的函数。
//!
//! 分为三组：
//! 1. **字符串匹配** — prefix/suffix/exact match, hide
//! 2. **显示名提取** — getDisplayName, getEmuName, getEmuPath
//! 3. **文件 I/O** — exists, putFile, getFile, putInt, getInt

use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::Path;

// ============================================================================
// 1. 字符串匹配
// ============================================================================

/// 大小写不敏感的前缀匹配 —— 对应 C 中的 `prefixMatch()`
///
/// ```c
/// return (strncasecmp(pre, str, strlen(pre)) == 0);
/// ```
///
/// ### 调用者
///
/// | 调用者 | 效果 |
/// |--------|------|
/// | `get_emu_name` | 判断路径是否在 ROMS_PATH 下，以提取模拟器标签 |
/// | `get_entries` | 归类（collation）匹配：同一主机不同变体的目录合并显示 |
/// | `load_recents` | 多碟游戏去重：同一 m3u 下的多张碟只保留最近一条 |
/// | `make_directory` | 判断是否是 Collections 伪目录 |
pub fn prefix_match(pre: &str, s: &str) -> bool {
    let pre_len = pre.len();
    if pre_len > s.len() {
        return false;
    }
    // 取 s 的前 pre_len 个字符，大小写不敏感比较
    s.chars()
        .zip(pre.chars())
        .take(pre_len)
        .all(|(sc, pc)| sc.eq_ignore_ascii_case(&pc))
}

/// 大小写不敏感的后缀匹配 —— 对应 C 中的 `suffixMatch()`
///
/// ```c
/// int offset = strlen(str) - strlen(suf);
/// return (offset >= 0 && strncasecmp(suf, str + offset, len) == 0);
/// ```
///
/// ### 调用者
///
/// | 调用者 | 效果 |
/// |--------|------|
/// | `hide` | 判断文件名是否以 `.disabled` 结尾 |
/// | `is_pak` / `is_m3u` / `is_cue` | 判断文件扩展名类型 |
/// | `get_display_name_with_platform` | 去除路径末尾的平台标签 |
/// | `open_rom`（未来） | 判断 ROM 路径是否已是 m3u 文件 |
pub fn suffix_match(suf: &str, s: &str) -> bool {
    let suf_len = suf.len();
    let s_len = s.len();
    if suf_len > s_len {
        return false;
    }
    let offset = s_len - suf_len;
    s.chars()
        .skip(offset)
        .zip(suf.chars())
        .all(|(sc, pc)| sc.eq_ignore_ascii_case(&pc))
}

/// 大小写敏感精确匹配 —— 对应 C 中的 `exactMatch()`
///
/// 在 C 版本中，先比较长度再比较内容。Rust 的 `==` 自动做这个优化。
/// 额外保留了 C 版本的 NULL 检查语义：任一为 None 返回 false。
///
/// ### 调用者
///
/// | 调用者 | 效果 |
/// |--------|------|
/// | `hide` | 判断文件名是否恰好是 `map.txt` |
/// | `scan.rs` 各处 | Entry 路径去重、目录名比较 |
/// | `save_last` / `load_last`（未来） | 路径相等性判断以恢复导航位置 |
pub fn exact_match(s1: &str, s2: &str) -> bool {
    s1 == s2
}

/// 大小写不敏感的子串搜索 —— 对应 C 中的 `containsString()`
///
/// ```c
/// return strcasestr(haystack, needle) != NULL;
/// ```
///
/// ### 使用场景
///
/// 当前代码中尚未直接使用。保留以备后续需要搜索文件内容或路径的场景。
pub fn contains_string(haystack: &str, needle: &str) -> bool {
    haystack
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

/// 判断文件名是否应隐藏 —— 对应 C 中的 `hide()`
///
/// 隐藏规则：
/// - 以 `.` 开头（隐藏文件/Unix 约定）
/// - 以 `.disabled` 结尾
/// - 恰好是 "map.txt"
///
/// ```c
/// return file_name[0]=='.' || suffixMatch(".disabled", file_name) || exactMatch("map.txt", file_name);
/// ```
///
/// ### 调用者
///
/// 这是整个扫描系统的**核心过滤器**，几乎所有读取目录的函数都依赖它：
///
/// | 调用者 | 效果 |
/// |--------|------|
/// | `scan_dir` | 遍历目录时过滤隐藏条目 |
/// | `scan_dir_has_visible` | 判断目录是否有非隐藏内容 |
/// | `get_root` | 扫描 ROMS 和 Collections 时过滤 |
/// | `get_collection` | 读取收藏文件内容后过滤 |
/// | `apply_directory_maps` | map.txt 映射后过滤被标记为隐藏的条目 |
pub fn hide(file_name: &str) -> bool {
    if file_name.is_empty() {
        return true; // 空文件名 → 隐藏（安全处理）
    }
    file_name.starts_with('.')
        || suffix_match(".disabled", file_name)
        || file_name == "map.txt"
}

// ============================================================================
// 2. 显示名提取
// ============================================================================

/// 从完整路径中提取显示名 —— 对应 C 中的 `getDisplayName()`
///
/// 处理步骤：
/// 1. 如果以 "/PLATFORM" 结尾 → 去掉（Tools 路径的特殊处理）
/// 2. 提取文件名（去掉目录部分）
/// 3. 去除扩展名（1-4 字符 + 点，循环去除多层如 .p8.png）
/// 4. 去除末尾的 () 和 [] 括号内容（区域/版本标记）
/// 5. 如果名字被清空 → 恢复为第 3 步后的名字
/// 6. 去除末尾空白
///
/// ## 示例
///
/// | 输入 | 输出 |
/// |------|------|
/// | `Roms/GB/Zelda (World).gb` | `Zelda` |
/// | `Roms/PS/Final Fantasy VII (Disc 1).cue` | `Final Fantasy VII` |
/// | `Tools/rg35xx/Clock.pak` | `Clock` |
/// | `Roms/GBA/Game (USA) [v1.1].gba` | `Game` |
///
/// ### 调用者
///
/// 这是整个 UI 显示名的**唯一入口**。所有 Entry 和 Directory 的名字都通过此函数生成：
///
/// | 调用者 | 效果 |
/// |--------|------|
/// | `create_entry` | 构建 Entry 时生成显示名（ROM、目录、Pak 均经此处） |
/// | `make_directory` | 构建 Directory 时生成目录自身的显示名 |
/// | `make_unique_name` | 构造同名条目的区分名时作为基础显示名 |
pub fn get_display_name(path: &str) -> String {
    // 步骤 1：如果路径以 "/PLATFORM" 结尾，去除之
    // 这处理了 Tools/<PLATFORM>/ 的情况 —— 但这里我们不知道 PLATFORM 是什么
    // 所以这个逻辑实际上需要在调用方处理。
    // 在 Rust 版本中，我们在 get_entries 中处理这个。
    // 不过为了保持兼容，我们接受一个可选的 platform_tag 参数。

    // 步骤 2：提取文件名
    let path_obj = Path::new(path);
    let mut name = path_obj
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string();

    // 步骤 3：去除扩展名（循环去除 1-4 字符长的扩展名 + 点）
    // 这处理了 .p8.png 这样的双层扩展名
    let saved = name.clone();
    loop {
        let dot_pos = name.rfind('.');
        match dot_pos {
            Some(pos) => {
                let ext_len = name.len() - pos - 1; // 点之后的长度
                if (1..=4).contains(&ext_len) {
                    name.truncate(pos);
                } else {
                    break; // 扩展名太长或太短（不是真正的扩展名）
                }
            }
            None => break,
        }
    }

    // 步骤 4：去除末尾的圆括号和方括号内容
    // 例如 "Zelda (World)" → "Zelda"
    //      "Game [v1.1]" → "Game"
    let after_ext = name.clone();
    loop {
        let paren = name.rfind('(');
        let bracket = name.rfind('[');
        let pos = match (paren, bracket) {
            (Some(p), Some(b)) => Some(p.max(b)),
            (Some(p), None) => Some(p),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        match pos {
            Some(p) if p > 0 => {
                name.truncate(p);
            }
            _ => break,
        }
    }

    // 步骤 5：如果名字被清空，恢复之
    if name.trim().is_empty() {
        name = after_ext;
    }
    if name.trim().is_empty() {
        name = saved;
    }

    // 步骤 6：去除末尾空白
    name = name.trim_end().to_string();

    name
}

/// 带平台标签的显示名提取
///
/// 对应 C 中 `getDisplayName` 的第一步：如果路径以 "/PLATFORM" 结尾则去除。
/// 例如 `Tools/rg35xx/Clock.pak` → 先变成 `Tools/Clock.pak` → 再提取 "Clock"
///
/// ### 调用者
/// `create_entry_with_platform` → 为 Tools 目录下的 Pak 生成不包含平台后缀的显示名
pub fn get_display_name_with_platform(path: &str, platform_tag: &str) -> String {
    let platform_suffix = format!("/{}", platform_tag);
    let path = if suffix_match(&platform_suffix, path) {
        // 去掉平台后缀
        &path[..path.len() - platform_suffix.len()]
    } else {
        path
    };
    get_display_name(path)
}

/// 从 ROM 路径提取模拟器标签 —— 对应 C 中的 `getEmuName()`
///
/// 算法：
/// 1. 如果路径在 ROMS_PATH 下 → 提取 Roms 子目录名（如 "Game Boy (GB)"）
/// 2. 从目录名末尾的括号中提取标签（如 "GB"）
///
/// ## 示例
///
/// | 输入 | roms_path | 输出 |
/// |------|-----------|------|
/// | `/sdcard/Roms/Game Boy (GB)/Zelda.gb` | `/sdcard/Roms` | `GB` |
/// | `/sdcard/Roms/Sega Genesis (MD)/Sonic.bin` | `/sdcard/Roms` | `MD` |
/// | `/sdcard/Tools/rg35xx/Clock.pak` | `/sdcard/Roms` | `Clock` |
///
/// ### 调用者
///
/// 模拟器标签是 ROM → Pak 映射的关键桥梁：
///
/// | 调用者 | 效果 |
/// |--------|------|
/// | `has_roms` | 从主机目录名提取标签 → 用 `has_emu` 验证模拟器是否存在 |
/// | `load_recents` | 从最近游戏的 ROM 路径提取标签 → 验证模拟器是否仍可用 |
/// | `open_rom`（未来） | 确定该 ROM 对应哪个模拟器 Pak |
/// | `ready_resume`（未来） | 构建存档状态文件路径时需要标签 |
pub fn get_emu_name(path: &str, roms_path: &str) -> String {
    let mut name = path.to_string();

    // 步骤 1：如果路径在 ROMS_PATH 下，提取 Roms 子目录名
    if prefix_match(roms_path, &name) {
        let after_roms = &name[roms_path.len()..];
        let trimmed = after_roms.trim_start_matches('/');
        // 取第一级目录名
        if let Some(slash_pos) = trimmed.find('/') {
            name = trimmed[..slash_pos].to_string();
        } else {
            name = trimmed.to_string();
        }
    }

    // 步骤 2：从末尾括号中提取标签
    // 例如 "Game Boy (GB)" → "GB"
    if let Some(paren_start) = name.rfind('(') {
        let after_paren = &name[paren_start + 1..];
        if let Some(paren_end) = after_paren.rfind(')') {
            name = after_paren[..paren_end].to_string();
        }
    }

    name
}

/// 根据模拟器标签构建模拟器启动路径 —— 对应 C 中的 `getEmuPath()`
///
/// 按优先级尝试两个位置：
/// 1. `<SDCARD>/Emus/<PLATFORM>/<emu>.pak/launch.sh` — 平台专属 Pak
/// 2. `<PAKS_PATH>/Emus/<emu>.pak/launch.sh` — 系统内置 Pak
///
/// 返回找到的第一个路径，如果都不存在则返回第二个路径（即使不存在）
///
/// ### 调用者
/// `open_rom`（未来） → 用户选择 ROM 后，用此路径构造启动命令
/// `has_emu` 也实现了类似的两级查找逻辑，但不调用此函数
pub fn get_emu_path(
    emu_name: &str,
    sdcard_path: &str,
    platform_tag: &str,
    paks_path: &str,
) -> String {
    // 位置 1：SD 卡上的平台专属模拟器
    let plat_path = format!(
        "{}/Emus/{}/{}.pak/launch.sh",
        sdcard_path, platform_tag, emu_name
    );
    if path_exists(&plat_path) {
        return plat_path;
    }

    // 位置 2：系统 Pak 目录
    let sys_path = format!("{}/Emus/{}.pak/launch.sh", paks_path, emu_name);
    sys_path
}

// ============================================================================
// 3. 字符串清理
// ============================================================================

/// 规范化换行符 —— 对应 C 中的 `normalizeNewline()`
///
/// Windows 风格 `\r\n` → Unix 风格 `\n`
/// 注意：在 C 版本中是在原 buffer 上修改，Rust 版本返回新 String
///
/// ### 调用者
/// `get_collection` / `load_recents` → 读取文本文件（收藏列表、最近游戏）时逐行规范化
pub fn normalize_newline(line: &str) -> String {
    let mut s = line.to_string();
    if s.ends_with("\r\n") {
        s.pop(); // 移除 \n
        s.pop(); // 移除 \r
        s.push('\n');
    }
    s
}

/// 去除末尾换行符 —— 对应 C 中的 `trimTrailingNewlines()`
///
/// ### 调用者
/// `get_collection` / `load_recents` → 读取文本文件的每一行后去除末尾 `\n`
pub fn trim_trailing_newlines(s: &str) -> &str {
    s.trim_end_matches('\n')
}

/// 跳过排序前缀 —— 对应 C 中的 `trimSortingMeta()`
///
/// 去除 `001) Game Name` 中的 `001) ` 前缀。
/// 这种前缀用于自定义 ROM 排序（用户可通过重命名控制顺序）。
///
/// 算法：
/// 1. 跳过前导数字
/// 2. 期望遇到 `)`
/// 3. 跳过后续空白
/// 4. 如果第 2 步不满足 → 回退到原始字符串
///
/// 返回去除前缀后的字符串切片。
///
/// ### 调用者
/// `main` 渲染循环（未来） → 在列表中显示条目名时去除序号前缀，使界面更干净
pub fn skip_sorting_meta(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut pos = 0;

    // 跳过前导数字
    while pos < bytes.len() && bytes[pos].is_ascii_digit() {
        pos += 1;
    }

    // 必须有数字被跳过，且下一个字符是 ')'
    if pos == 0 || pos >= bytes.len() || bytes[pos] != b')' {
        return s;
    }
    pos += 1; // 跳过 ')'

    // 跳过后续空白
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }

    &s[pos..]
}

// ============================================================================
// 4. 文件 I/O
// ============================================================================

/// 检查路径是否存在 —— 对应 C 中的 `exists()`
///
/// 同时检查文件和目录。
///
/// ### 调用者
/// **全局使用** —— 几乎所有扫描函数和 I/O 函数都依赖此函数。是文件系统操作的基础。
pub fn path_exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// 检查路径是否是文件且存在
pub fn file_exists(path: &str) -> bool {
    Path::new(path).is_file()
}

/// 检查路径是否是目录且存在
pub fn dir_exists(path: &str) -> bool {
    Path::new(path).is_dir()
}

/// 创建空文件 —— 对应 C 中的 `touch()`
///
/// ### 调用者
/// 用于创建标记文件（如 `enable-simple-mode`）或占位 `.keep` 文件
pub fn touch(path: &str) -> std::io::Result<()> {
    fs::File::create(path)?;
    Ok(())
}

/// 写入字符串到文件 —— 对应 C 中的 `putFile()`
///
/// 会覆盖已有内容。
///
/// ### 调用者
/// `queue_next`（未来） → 将下一阶段命令写入 `/tmp/next`
/// `save_last`（未来） → 将当前浏览路径写入 `/tmp/last.txt`
/// `save_recents`（未来） → 将最近游戏列表写入持久化文件
pub fn put_file(path: &str, contents: &str) -> std::io::Result<()> {
    fs::write(path, contents)
}

/// 读取文件内容到字符串 —— 对应 C 中的 `getFile()`
///
/// 注意：C 版本限制了读取长度（buffer_size - 1），Rust 版本读取整个文件。
///
/// ### 调用者
/// `load_last`（未来） → 读取 `/tmp/last.txt` 恢复上次浏览位置
/// `auto_resume`（未来） → 读取 `auto_resume.txt` 恢复被中断的游戏
/// `load_recents` → 读取 `recent.txt` 载入最近游戏列表
pub fn get_file(path: &str) -> std::io::Result<String> {
    fs::read_to_string(path)
}

/// 读取文件内容，带最大长度限制 —— 对应 C 中 `getFile()` 的截断行为
pub fn get_file_limited(path: &str, max_size: usize) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer)?;
    // 截断到 max_size - 1（为 null terminator 留空间，与 C 版本一致）
    let limit = max_size.saturating_sub(1);
    if buffer.len() > limit {
        buffer.truncate(limit);
    }
    Ok(buffer)
}

/// 写入整数到文件 —— 对应 C 中的 `putInt()`
///
/// ### 调用者
/// `open_rom`（未来） → 写入存档槽位号到 `/tmp/resume_slot.txt`
/// settings 操作（未来） → 写入音量/亮度等整数值
pub fn put_int(path: &str, value: i32) -> std::io::Result<()> {
    put_file(path, &value.to_string())
}

/// 从文件读取整数 —— 对应 C 中的 `getInt()`
///
/// 如果文件不存在或无法解析，返回 0。
///
/// ### 调用者
/// `platform` 实现 → 读取 `/sys/class/power_supply/battery/` 下的电池状态
/// settings 恢复（未来） → 读取存档槽位号
pub fn get_int(path: &str) -> i32 {
    match fs::read_to_string(path) {
        Ok(s) => s.trim().parse().unwrap_or(0),
        Err(_) => 0,
    }
}

/// 分配并读取文件（对应 C 中的 `allocFile`）
///
/// 在 Rust 中直接返回 `Option<String>` 即可，不需要手动管理内存。
///
/// ### 调用者
/// `show_version` 渲染（未来） → 读取 `version.txt` 和 `commits.txt` 以显示版本信息
pub fn alloc_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

// ============================================================================
// 5. 路径操作辅助
// ============================================================================

/// 获取路径的文件名部分
///
/// ### 调用者
/// `get_display_name` → 从路径中提取文件名
/// `get_emu_name` → 从 ROMS 路径中提取目录名
/// `scan.rs` 各函数 → 提取文件名用于 map.txt 匹配和同名检测
pub fn file_name(path: &str) -> Option<&str> {
    Path::new(path).file_name().and_then(OsStr::to_str)
}

/// 获取路径的父目录
///
/// ### 调用者
/// `is_console_dir` → 判断路径父目录是否是 ROMS_PATH
/// `find_m3u` → 构造 m3u 文件路径需要找到 ROM 的上级目录
/// `load_recents` → 多碟去重时需要获取碟文件所在目录
pub fn parent_dir(path: &str) -> Option<&str> {
    Path::new(path).parent().and_then(|p| p.to_str())
}

/// 检查路径是否以 `.pak` 结尾
///
/// ### 调用者
/// `scan_dir` → 判断目录是否是一个 Pak（`.pak` 扩展名的目录视为模拟器/工具包）
/// `get_recents_from_list` / `get_collection` → 判断最近游戏/收藏条目是 ROM 还是 Pak
pub fn is_pak(path: &str) -> bool {
    suffix_match(".pak", path)
}

/// 检查路径是否以 `.m3u` 结尾
///
/// ### 调用者
/// `open_rom`（未来） → 如果用户选择的 ROM 本身是 m3u 文件，需要获取第一张碟
pub fn is_m3u(path: &str) -> bool {
    suffix_match(".m3u", path)
}

/// 检查路径是否以 `.cue` 结尾
///
/// ### 调用者
/// `find_cue` / `open_directory`（未来） → 检测 PS1 光盘映像的 cue 文件以判断是否自动启动
pub fn is_cue(path: &str) -> bool {
    suffix_match(".cue", path)
}

/// 将 DOS 风格的 `\r\n` 转换为 Unix `\n`
pub fn normalize_line_endings(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// 分割文件内容为行，去除空行和 `#` 注释行
///
/// ### 调用者
/// `get_collection` / `get_discs` → 读取收藏列表和 m3u 文件时使用
pub fn read_lines_filtered(path: &str) -> std::io::Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(String::from)
        .collect())
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ==== 字符串匹配 ====

    #[test]
    fn test_prefix_match() {
        assert!(prefix_match("hello", "Hello World"));
        assert!(prefix_match("HELLO", "hello world"));
        assert!(!prefix_match("hello", "world hello"));
        assert!(prefix_match("", "anything"));
        assert!(!prefix_match("hello", "hel"));
    }

    #[test]
    fn test_suffix_match() {
        assert!(suffix_match(".gb", "zelda.gb"));
        assert!(suffix_match(".GB", "zelda.gb"));
        assert!(suffix_match(".pak", "Emus/GB.pak"));
        assert!(!suffix_match(".gba", "zelda.gb"));
        assert!(suffix_match("", "anything"));
    }

    #[test]
    fn test_exact_match() {
        assert!(exact_match("hello", "hello"));
        assert!(!exact_match("Hello", "hello")); // 大小写敏感
        assert!(!exact_match("hello", "hello!"));
    }

    #[test]
    fn test_hide() {
        assert!(hide(".hidden"));
        assert!(hide(".git"));
        assert!(hide("file.disabled"));
        assert!(hide("map.txt"));
        assert!(!hide("zelda.gb"));
        assert!(!hide("Game Boy (GB)"));
        assert!(hide("")); // 空文件名应隐藏
    }

    // ==== 显示名提取 ====

    #[test]
    fn test_get_display_name_basic() {
        assert_eq!(get_display_name("zelda.gb"), "zelda");
        assert_eq!(get_display_name("Zelda.gb"), "Zelda");
        assert_eq!(get_display_name("game.p8.png"), "game"); // 双层扩展名
    }

    #[test]
    fn test_get_display_name_remove_parens() {
        assert_eq!(
            get_display_name("Game (World).gb"),
            "Game"
        );
        assert_eq!(
            get_display_name("Game (USA) [v1.1].gba"),
            "Game"
        );
        assert_eq!(
            get_display_name("Final Fantasy VII (Disc 1).cue"),
            "Final Fantasy VII"
        );
    }

    #[test]
    fn test_get_display_name_no_ext() {
        // 没有扩展名的情况
        let result = get_display_name("README");
        assert!(!result.is_empty());
    }

    // ==== 模拟器名提取 ====

    #[test]
    fn test_get_emu_name_from_roms() {
        let roms = "/mnt/sdcard/Roms";
        assert_eq!(
            get_emu_name("/mnt/sdcard/Roms/Game Boy (GB)/Zelda.gb", roms),
            "GB"
        );
        assert_eq!(
            get_emu_name("/mnt/sdcard/Roms/Sega Genesis (MD)/Sonic.bin", roms),
            "MD"
        );
        assert_eq!(
            get_emu_name("/mnt/sdcard/Roms/PlayStation (PS)/Game.cue", roms),
            "PS"
        );
    }

    #[test]
    fn test_get_emu_name_from_tools() {
        let roms = "/mnt/sdcard/Roms";
        let result = get_emu_name("/mnt/sdcard/Tools/rg35xx/Clock.pak", roms);
        // Tools 路径不在 ROMS_PATH 下，且没有括号 → 返回原值不变
        // 这与 C 版本行为一致：前缀不匹配 ROMS_PATH 且无括号时不作任何修改
        assert_eq!(result, "/mnt/sdcard/Tools/rg35xx/Clock.pak");
    }

    // ==== 字符串清理 ====

    #[test]
    fn test_normalize_newline() {
        assert_eq!(normalize_newline("hello\r\n"), "hello\n");
        assert_eq!(normalize_newline("hello\n"), "hello\n");
        assert_eq!(normalize_newline("hello"), "hello");
    }

    #[test]
    fn test_skip_sorting_meta() {
        assert_eq!(skip_sorting_meta("001) Game Name"), "Game Name");
        assert_eq!(skip_sorting_meta("1) Zelda"), "Zelda");
        assert_eq!(skip_sorting_meta("Game Name"), "Game Name"); // 没有前缀
        assert_eq!(skip_sorting_meta("123"), "123"); // 有数字但没有 )
    }

    // ==== 文件 I/O ====

    #[test]
    fn test_put_and_get_file() {
        let path = "/tmp/minui_test_file.txt";
        put_file(path, "hello world").unwrap();
        assert_eq!(get_file(path).unwrap(), "hello world");
        assert!(path_exists(path));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_put_and_get_int() {
        let path = "/tmp/minui_test_int.txt";
        put_int(path, 42).unwrap();
        assert_eq!(get_int(path), 42);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_get_int_missing_file() {
        assert_eq!(get_int("/tmp/definitely_does_not_exist_12345"), 0);
    }

    #[test]
    fn test_file_exists() {
        assert!(file_exists("/tmp/minui_test_file.txt") == false);
    }

    #[test]
    fn test_path_helpers() {
        assert_eq!(file_name("/mnt/sdcard/Roms/GB/Zelda.gb"), Some("Zelda.gb"));
        assert_eq!(parent_dir("/mnt/sdcard/Roms/GB/Zelda.gb"), Some("/mnt/sdcard/Roms/GB"));
        assert!(is_pak("Emus/GB.pak"));
        assert!(!is_pak("Zelda.gb"));
        assert!(is_m3u("Final Fantasy.m3u"));
    }
}
