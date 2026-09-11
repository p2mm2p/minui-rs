//! 游戏文件组织判断
//!
//! 提供"SD 卡上这个游戏文件/目录是什么性质"的判断函数：多碟游戏
//! （`.m3u`）、单碟音轨游戏（`.cue`）、模拟器（`.pak`）安装状态、
//! 多碟游戏的第一个碟片。
//!
//! 对应原 C `minui.c` 中 Recent 区前后的检测函数族（`hasM3u`/`hasCue`/
//! `hasEmu`/`getFirstDisc`，minui.c:475-516/:837-861）。本模块只承载
//! 文件组织判断，目录扫描/索引（browser.rs）与碟片条目构造
//! （`getDiscs`）不在此处。
//!
//! ## 依赖方向
//!
//! 本模块只依赖 `common`（路径常量/工具），不依赖 minui 的任何业务
//! 模块（launch/recents/browser）。minarch 的 m3u 检测（`Game_open`）
//! 语义独立（设置 `game.m3u_path` vs 本模块的多碟判断），不共享。
//!

#![cfg_attr(not(feature = "tg5040"), allow(dead_code))] // 无平台 feature（测试编译）时保留 allow

use common::utils::exists;

/// 查找 ROM 所属多碟游戏的 m3u 播放列表路径
///
/// 判定规则：取 ROM 的父目录，检查 `{父目录}/{父目录名}.m3u` 是否存在。
/// 对应原 C `hasM3u()`（`minui.c:488-516`）。扩展名大小写敏感（C 的
/// 字面拼接语义——只有 `.M3U` 不算命中）。
///
/// # 参数
///
/// - `rom_path`:ROM 文件路径（如 `"/mnt/SDCARD/Roms/SFC/Final Fantasy VII/Disc 1.sfc"`）
///
/// # 返回值
///
/// `Some(m3u 路径)`——该 ROM 属于多碟游戏（父目录存在同名 `.m3u`）；
/// `None`——不是多碟游戏，或路径不含 `/` 分隔符（无法定位父目录）。
pub(crate) fn find_m3u(rom_path: &str) -> Option<String> {
    let parent = rom_path.rsplit_once('/').map(|(p, _)| p)?;
    let dir_name = parent.rsplit_once('/').map(|(_, d)| d)?;
    let m3u_path = format!("{parent}/{dir_name}.m3u");
    exists(&m3u_path).then_some(m3u_path)
}

/// 查找目录的同名 .cue 音轨文件路径
///
/// 判定规则：检查 `{dir_path}/{目录名}.cue` 是否存在。对应原 C `hasCue()`
/// （`minui.c:483-487`）。目录路径带/不带末尾 `/` 均可（实现先 `trim_end_matches('/')`
/// ——C 版对带尾斜杠路径会拼出双斜杠路径，Rust 版规范化，语义等价）。
///
/// # 参数
///
/// - `dir_path`:游戏目录路径（如 `"/mnt/SDCARD/Roms/PS/Final Fantasy VII"`）
///
/// # 返回值
///
/// `Some(cue 路径)`——目录是单碟音轨游戏（存在同名 `.cue`）；
/// `None`——不是音轨游戏，或路径不含 `/` 分隔符（无法定位目录名）。
pub(crate) fn find_cue(dir_path: &str) -> Option<String> {
    let trimmed = dir_path.trim_end_matches('/');
    let dir_name = trimmed.rsplit_once('/').map(|(_, d)| d)?;
    let cue_path = format!("{trimmed}/{dir_name}.cue");
    exists(&cue_path).then_some(cue_path)
}

/// 模拟器（`.pak`）是否已安装
///
/// 检查两个候选路径**任一**存在：
/// 1. 优先：`{sdcard_path}/Emus/{platform}/{emu_name}.pak/launch.sh`
/// 2. 回退：`{paks_path}/Emus/{emu_name}.pak/launch.sh`
///
/// 对应原 C `hasEmu()`（`minui.c:475-482`）。
///
/// # 参数
///
/// - `emu_name`:模拟器名称（`.pak` 标签，如 `"GB"`，来自 `get_emu_name`）
/// - `sdcard_path`:SD 卡根路径（如 `"/mnt/SDCARD"`，来自 `Platform::SDCARD_PATH`）
/// - `platform`:平台代码（如 `"tg5040"`，来自 `Platform::PLATFORM`）
/// - `paks_path`:平台 paks 目录（如 `"/mnt/SDCARD/.system/tg5040/paks"`）
///
/// # 返回值
///
/// `true`——两个候选路径任一存在（模拟器已安装）；`false`——均不存在。
pub(crate) fn has_emu(emu_name: &str, sdcard_path: &str, platform: &str, paks_path: &str) -> bool {
    let primary = format!("{sdcard_path}/Emus/{platform}/{emu_name}.pak/launch.sh");
    if exists(&primary) {
        return true;
    }
    let fallback = format!("{paks_path}/Emus/{emu_name}.pak/launch.sh");
    exists(&fallback)
}

/// 读取 m3u 文件的第一个可用碟片路径
///
/// 读 m3u 的第一行（跳过空行），拼接 `{m3u 父目录}/{行内容}`，检查存在性。
/// 对应原 C `getFirstDisc()`（`minui.c:837-861`）。
///
/// **C 语义细节**：只尝试第一非空行——无论该行对应文件是否存在都立即
/// 返回（`break`），不继续尝试后续行。
///
/// # 参数
///
/// - `m3u_path`:m3u 播放列表路径（如 `"/mnt/SDCARD/Roms/SFC/Final Fantasy VII/Final Fantasy VII.m3u"`）
///
/// # 返回值
///
/// `Some(碟片路径)`——第一非空行对应文件存在；`None`——m3u 不存在、
/// 为空、或第一非空行对应文件不存在。
pub(crate) fn get_first_disc(m3u_path: &str) -> Option<String> {
    let base = m3u_path.rsplit_once('/').map(|(p, _)| format!("{p}/"))?;
    let content = std::fs::read_to_string(m3u_path).ok()?;
    for line in content.lines() {
        if line.is_empty() {
            continue;
        }
        let disc_path = format!("{base}{line}");
        return exists(&disc_path).then_some(disc_path);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 生成测试根目录（正斜杠路径——Windows 的 `std::fs` 接受 `/`，
    /// 且与真实设备上的路径格式一致）
    fn temp_root(name: &str) -> String {
        let base = std::env::temp_dir().to_string_lossy().replace('\\', "/");
        format!("{base}/minui_disc_{}_{name}", std::process::id())
    }

    // ── find_m3u ────────────────────────────────

    #[test]
    fn find_m3u_hit() {
        let root = temp_root("m3u_hit");
        let game_dir = format!("{root}/Roms/SFC/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(format!("{game_dir}/Final Fantasy VII.m3u"), "Disc 1.sfc\n").unwrap();
        fs::write(format!("{game_dir}/Disc 1.sfc"), "x").unwrap();

        let result = find_m3u(&format!("{game_dir}/Disc 1.sfc"));
        assert_eq!(
            result.as_deref(),
            Some(format!("{game_dir}/Final Fantasy VII.m3u").as_str())
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn find_m3u_miss() {
        let root = temp_root("m3u_miss");
        let game_dir = format!("{root}/Roms/SFC/Dragon Quest");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(format!("{game_dir}/Dragon Quest.sfc"), "x").unwrap();

        let result = find_m3u(&format!("{game_dir}/Dragon Quest.sfc"));
        assert_eq!(result, None);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    // 大小写敏感是 Linux/真实设备语义。仅门控 Linux：macOS APFS 与 Windows
    // NTFS 均大小写不敏感——写入 `.M3U` 即创建了大小写不敏感视图下的
    // `.m3u`（同一文件），`exists` 恒真，无法构造"仅大写扩展名"的对照场景。
    #[cfg(target_os = "linux")]
    fn find_m3u_extension_case_sensitive() {
        // C 的 `strcpy ".m3u"` 是字面拼接——只有大写 `.M3U` 不算命中
        let root = temp_root("m3u_case");
        let game_dir = format!("{root}/Roms/SFC/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(format!("{game_dir}/Final Fantasy VII.M3U"), "Disc 1.sfc\n").unwrap();
        fs::write(format!("{game_dir}/Disc 1.sfc"), "x").unwrap();

        let result = find_m3u(&format!("{game_dir}/Disc 1.sfc"));
        assert_eq!(result, None);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn find_m3u_no_slash_returns_none() {
        // 路径不含 `/` 无法定位父目录——安全返回 None（C 在此场景会崩溃，
        // Rust 版以 `None` 替代，属于安全化改进）
        assert_eq!(find_m3u("game.gb"), None);
    }

    // ── find_cue ────────────────────────────────

    #[test]
    fn find_cue_hit() {
        let root = temp_root("cue_hit");
        let game_dir = format!("{root}/Roms/PS/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(
            format!("{game_dir}/Final Fantasy VII.cue"),
            "FILE \"x.bin\" BINARY\n",
        )
        .unwrap();

        let result = find_cue(&game_dir);
        assert_eq!(
            result.as_deref(),
            Some(format!("{game_dir}/Final Fantasy VII.cue").as_str())
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn find_cue_hit_with_trailing_slash() {
        // 目录路径带末尾 `/`（C 的 entry->path 即此形态）也应命中
        let root = temp_root("cue_trailing");
        let game_dir = format!("{root}/Roms/PS/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(
            format!("{game_dir}/Final Fantasy VII.cue"),
            "FILE \"x.bin\" BINARY\n",
        )
        .unwrap();

        let result = find_cue(&format!("{game_dir}/"));
        assert_eq!(
            result.as_deref(),
            Some(format!("{game_dir}/Final Fantasy VII.cue").as_str())
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn find_cue_miss() {
        let root = temp_root("cue_miss");
        let game_dir = format!("{root}/Roms/PS/Dragon Quest");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(format!("{game_dir}/Dragon Quest.bin"), "x").unwrap();

        let result = find_cue(&game_dir);
        assert_eq!(result, None);

        fs::remove_dir_all(&root).unwrap();
    }

    // ── has_emu ────────────────────────────────

    #[test]
    fn has_emu_primary_hit() {
        let root = temp_root("emu_primary");
        let sdcard = format!("{root}/SDCARD");
        let platform = "tg5040";
        let pak = format!("{sdcard}/Emus/{platform}/GB.pak/launch.sh");
        fs::create_dir_all(std::path::Path::new(&pak).parent().unwrap()).unwrap();
        fs::write(&pak, "#!/bin/sh\n").unwrap();

        let result = has_emu(
            "GB",
            &sdcard,
            platform,
            &format!("{sdcard}/.system/{platform}/paks"),
        );
        assert!(result);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn has_emu_fallback_hit() {
        let root = temp_root("emu_fallback");
        let sdcard = format!("{root}/SDCARD");
        let platform = "tg5040";
        let paks = format!("{sdcard}/.system/{platform}/paks");
        let pak = format!("{paks}/Emus/GB.pak/launch.sh");
        fs::create_dir_all(std::path::Path::new(&pak).parent().unwrap()).unwrap();
        fs::write(&pak, "#!/bin/sh\n").unwrap();

        // 优先候选不存在，回退候选命中
        let result = has_emu("GB", &sdcard, platform, &paks);
        assert!(result);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn has_emu_both_miss() {
        let root = temp_root("emu_miss");
        let sdcard = format!("{root}/SDCARD");
        let platform = "tg5040";
        let paks = format!("{sdcard}/.system/{platform}/paks");
        fs::create_dir_all(&paks).unwrap();

        let result = has_emu("GB", &sdcard, platform, &paks);
        assert!(!result);

        fs::remove_dir_all(&root).unwrap();
    }

    // ── get_first_disc ─────────────────────────

    #[test]
    fn get_first_disc_first_line() {
        let root = temp_root("disc_first");
        let game_dir = format!("{root}/Roms/SFC/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(
            format!("{game_dir}/Final Fantasy VII.m3u"),
            "Disc 1.sfc\nDisc 2.sfc\n",
        )
        .unwrap();
        fs::write(format!("{game_dir}/Disc 1.sfc"), "x").unwrap();
        fs::write(format!("{game_dir}/Disc 2.sfc"), "x").unwrap();

        let result = get_first_disc(&format!("{game_dir}/Final Fantasy VII.m3u"));
        assert_eq!(
            result.as_deref(),
            Some(format!("{game_dir}/Disc 1.sfc").as_str())
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn get_first_disc_empty_m3u() {
        let root = temp_root("disc_empty");
        let game_dir = format!("{root}/Roms/SFC/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(format!("{game_dir}/Final Fantasy VII.m3u"), "").unwrap();

        let result = get_first_disc(&format!("{game_dir}/Final Fantasy VII.m3u"));
        assert_eq!(result, None);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn get_first_disc_first_line_missing() {
        // C 语义：第一非空行对应文件不存在 → 立即返回（不尝试后续行）
        let root = temp_root("disc_missing");
        let game_dir = format!("{root}/Roms/SFC/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(
            format!("{game_dir}/Final Fantasy VII.m3u"),
            "Ghost Disc.sfc\nDisc 2.sfc\n",
        )
        .unwrap();
        fs::write(format!("{game_dir}/Disc 2.sfc"), "x").unwrap();

        let result = get_first_disc(&format!("{game_dir}/Final Fantasy VII.m3u"));
        assert_eq!(result, None);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn get_first_disc_skips_blank_lines() {
        let root = temp_root("disc_blank");
        let game_dir = format!("{root}/Roms/SFC/Final Fantasy VII");
        fs::create_dir_all(&game_dir).unwrap();
        fs::write(
            format!("{game_dir}/Final Fantasy VII.m3u"),
            "\nDisc 1.sfc\n",
        )
        .unwrap();
        fs::write(format!("{game_dir}/Disc 1.sfc"), "x").unwrap();

        let result = get_first_disc(&format!("{game_dir}/Final Fantasy VII.m3u"));
        assert_eq!(
            result.as_deref(),
            Some(format!("{game_dir}/Disc 1.sfc").as_str())
        );

        fs::remove_dir_all(&root).unwrap();
    }
}
